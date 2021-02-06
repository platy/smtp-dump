use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use dotenv::dotenv;
use file_lock::FileLock;
use git2::{Commit, Repository, Signature};
use gitgov_rs::{email_update::GovUkChange, git::CommitBuilder, retrieve_doc};
use std::{
    collections::VecDeque,
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
};
use url::Url;

struct MailHandler {
    peer_addr: SocketAddr,
    inbox: PathBuf,
    data: Option<EmailWrite>,
}

impl mailin::Handler for MailHandler {
    fn helo(&mut self, ip: std::net::IpAddr, domain: &str) -> mailin::Response {
        println!("{}: HELO {} {}", self.peer_addr, ip, domain);
        mailin::response::OK
    }

    fn mail(&mut self, ip: std::net::IpAddr, domain: &str, from: &str) -> mailin::Response {
        println!("{}: MAIL {}", self.peer_addr, from);
        let from_match = dotenv::var("FROM_FILTER")
            .ok()
            .map(|from_filter| from.contains(&from_filter));
        if from_match == Some(false) {
            println!(
                "{}: {}({}) tried to send some junk, purporting to be {}",
                self.peer_addr, domain, ip, from
            );
            mailin::response::NO_SERVICE
        } else {
            mailin::response::OK
        }
    }

    fn rcpt(&mut self, to: &str) -> mailin::Response {
        println!("{}: RCPT {}", self.peer_addr, to);
        mailin::response::OK
    }

    fn data_start(&mut self, _domain: &str, from: &str, _is8bit: bool, to: &[String]) -> mailin::Response {
        let email_path = inbox_path_for_email(&self.inbox, from, to);
        match EmailWrite::create(email_path) {
            Ok(writer) => {
                println!(
                    "{}: Writing email to {}",
                    self.peer_addr,
                    writer.path.to_str().unwrap_or_default()
                );
                self.data = Some(writer);
                mailin::response::OK
            }
            Err(err) => {
                println!("{}: Error mapping email envelope to inbox : {}", self.peer_addr, err);
                mailin::response::INTERNAL_ERROR
            }
        }
    }

    fn data(&mut self, buf: &[u8]) -> io::Result<()> {
        if let Some(writer) = &mut self.data {
            writer.write_all(buf)
        } else {
            Err(io::ErrorKind::NotConnected.into())
        }
    }

    fn data_end(&mut self) -> mailin::Response {
        if let Some(mut writer) = self.data.take() {
            match writer.flush() {
                Ok(()) => mailin::response::OK,
                Err(err) => {
                    println!("Error flushing : {}", err);
                    mailin::response::INTERNAL_ERROR
                }
            }
        } else {
            mailin::response::INTERNAL_ERROR
        }
    }
}

struct EmailWrite {
    path: PathBuf,
    lock: FileLock,
}

impl EmailWrite {
    fn create(path: PathBuf) -> Result<Self> {
        fs::create_dir_all(path.parent().unwrap())?;
        Ok(EmailWrite {
            lock: FileLock::lock(&path.to_str().unwrap(), true, true)?,
            path,
        })
    }
}

impl Write for EmailWrite {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.lock.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.lock.file.flush()
    }
}

impl Drop for EmailWrite {
    fn drop(&mut self) {
        println!("Finished writing {}", self.path.to_string_lossy());
    }
}

fn inbox_path_for_email(inbox: &PathBuf, from: &str, to: &[String]) -> PathBuf {
    let from_domain = from.split('@').nth(1);
    inbox
        .join(from_domain.unwrap_or(from))
        .join(to.join(","))
        .join(Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true))
        .with_extension("eml")
}

/// accepts emails from gov.uk and saves them in `inbox/{from}/{to}/{datetime}.eml
fn receive_updates_on_socket(mut stream: TcpStream, remote_addr: SocketAddr, inbox: impl AsRef<Path>) -> Result<()> {
    let peer_addr = stream.peer_addr()?;
    let handler = MailHandler {
        peer_addr,
        inbox: inbox.as_ref().to_path_buf(),
        data: None,
    };
    let mut session = mailin::SessionBuilder::new("gitgov").build(remote_addr.ip(), handler);
    session.greeting().write_to(&mut stream)?;

    let mut buf_read = BufReader::new(stream.try_clone()?);
    let mut command = String::new();

    loop {
        command.clear();
        let len = buf_read.read_line(&mut command)?;
        let command = if len == 0 {
            break;
        } else {
            &command[..]
        };
        let result = session.process(&command.as_bytes());
        match result.action {
            mailin::Action::Close => {
                println!("{}: CLOSE", peer_addr);
                result.write_to(&mut stream)?;
                break;
            }
            mailin::Action::UpgradeTls => bail!("TLS requested"),
            mailin::Action::NoReply => continue,
            mailin::Action::Reply => result.write_to(&mut stream).context(format!(
                "{}: Writing SMTP reply failed when responding to '{}' with '{:?}'",
                peer_addr, command, result
            ))?,
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    dotenv()?;
    const EMAILS_FROM_GOVUK_PATH: &str = "inbox/mail.notifications.service.gov.uk";
    const ARCHIVE_DIR: &str = "outbox";
    let repo_path = dotenv::var("REPO")?;
    let reference = dotenv::var("REF")?;
    fs::create_dir_all(EMAILS_FROM_GOVUK_PATH)
        .context(format!("Error trying to create dir {}", EMAILS_FROM_GOVUK_PATH))?;
    fs::create_dir_all(ARCHIVE_DIR).context(format!("Error trying to create dir {}", ARCHIVE_DIR))?;

    if dotenv::var("DISABLE_PROCESS_UPDATES").is_err() {
        push(&repo_path)?;
    }

    let socket = TcpListener::bind("0.0.0.0:25")?;
    loop {
        if dotenv::var("DISABLE_PROCESS_UPDATES").is_err() {
            let count = process_updates_in_dir(EMAILS_FROM_GOVUK_PATH, ARCHIVE_DIR, &repo_path, &reference)
                .expect("the processing fails, the repo may be unclean");
            if count > 0 {
                println!("Processed {} update emails, pushing", count);
                push(&repo_path).unwrap_or_else(|err| println!("Push failed : {}", err));
            }
        }

        let (stream, remote_addr) = socket.accept()?;
        if let Err(err) = receive_updates_on_socket(stream, remote_addr, "inbox") {
            println!("Closed SMTP session due to error : {}", err);
        }
    }
}

fn push(repo_base: impl AsRef<Path>) -> Result<()> {
    let mut remote_callbacks = git2::RemoteCallbacks::new();
    remote_callbacks.credentials(|_url, username_from_url, _allowed_types| {
        git2::Cred::ssh_key(
            username_from_url.unwrap(),
            None,
            std::path::Path::new(&format!("{}/.ssh/id_rsa", std::env::var("HOME").unwrap())),
            None,
        )
    });
    let repo = Repository::open(repo_base).context("Opening repo")?;
    let mut remote = repo.find_remote("origin")?;
    remote.push(
        &["refs/heads/main"],
        Some(git2::PushOptions::new().remote_callbacks(remote_callbacks)),
    )?;
    Ok(())
}

fn process_updates_in_dir(
    in_dir: impl AsRef<Path>,
    out_dir: impl AsRef<Path>,
    repo: impl AsRef<Path>,
    reference: &str,
) -> Result<u32> {
    let mut count = 0;
    for to_inbox in fs::read_dir(in_dir)? {
        let to_inbox = to_inbox?;
        if to_inbox.metadata()?.is_dir() {
            for email in fs::read_dir(to_inbox.path())? {
                let email = email?;
                process_email_update_file(to_inbox.file_name(), &email, &out_dir, &repo, reference).context(
                    format!("Failed processing {}", email.path().to_str().unwrap_or_default()),
                )?;
                count += 1;
            }
        }
    }
    Ok(count)
}

fn process_email_update_file(
    to_dir_name: impl AsRef<Path>,
    dir_entry: &fs::DirEntry,
    out_dir: impl AsRef<Path>,
    repo_base: impl AsRef<Path>,
    reference: &str,
) -> Result<()> {
    let data = {
        let mut lock = FileLock::lock(dir_entry.path().to_str().context("error")?, true, false)
            .context("Locking file email file")?;
        let mut bytes = Vec::with_capacity(lock.file.metadata().map(|m| m.len() as usize + 1).unwrap_or(0));
        lock.file.read_to_end(&mut bytes).context("Reading email file")?;
        bytes
    };
    let updates = GovUkChange::from_eml(&String::from_utf8(data)?).context("Parsing email")?;
    let repo = Repository::open(repo_base).context("Opening repo")?;
    let mut parent = Some(repo.find_reference(reference)?.peel_to_commit()?);
    for change in &updates {
        parent = Some(handle_change(change, &repo, parent).context(format!("Processing change {:?}", change))?);
    }
    // successfully handled, 'commit' the new commits by updating the reference and then move email to outbox
    if let Some(commit) = parent {
        let _ref = repo.reference(
            reference,
            commit.id(),
            true,
            &format!("Added updates from {:?}", dir_entry.path()),
        )?;
    }
    let done_path = out_dir.as_ref().join(to_dir_name).join(dir_entry.file_name());
    fs::create_dir_all(done_path.parent().unwrap()).context("Creating outbox dir")?;
    fs::rename(dir_entry.path(), &done_path).context(format!(
        "Renaming file {} to {}",
        dir_entry.path().to_str().unwrap_or_default(),
        &done_path.to_str().unwrap_or_default()
    ))?;
    Ok(())
}

fn handle_change<'repo>(
    GovUkChange {
        url,
        change,
        updated_at,
        category,
    }: &GovUkChange,
    repo: &'repo Repository,
    parent: Option<Commit<'repo>>,
) -> Result<Commit<'repo>> {
    let mut commit_builder = CommitBuilder::new(&repo, parent)?;

    fetch_change(url, |path, bytes| {
        // write the blob
        let oid = repo.blob(bytes)?;
        commit_builder.add_to_tree(path.to_str().unwrap(), oid, 0o100644)
    })?;

    let message = format!(
        "{}: {}{}",
        updated_at,
        change,
        category.as_ref().map(|c| format!(" [{}]", c)).unwrap_or_default()
    );
    let govuk_sig = Signature::now("Gov.uk", "info@gov.uk")?;
    let gitgov_sig = Signature::now("Gitgov", "gitgov@njk.onl")?;
    Ok(commit_builder.commit(&govuk_sig, &gitgov_sig, &message)?)
}

fn fetch_change(url: &Url, mut write_out: impl FnMut(PathBuf, &[u8]) -> Result<()>) -> Result<()> {
    let mut urls = VecDeque::new();
    urls.push_back(url.to_owned());

    while let Some(url) = urls.pop_front() {
        if url.host_str() != Some("www.gov.uk") {
            println!("Ignoring link to offsite document : {}", &url);
            continue;
        }
        let doc = retrieve_doc(&url)?;
        urls.extend(doc.content.attachments().unwrap_or_default().iter().cloned());

        let mut path = PathBuf::from(doc.url.path());
        if doc.content.is_html() {
            assert!(path.set_extension("html"));
        }
        println!("Writing doc to : {}", path.to_str().unwrap());
        write_out(path, doc.content.as_bytes())?
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::receive_updates_on_socket;
    use crate::handle_change;
    use anyhow::Result;
    use git2::{Repository, Signature};
    use gitgov_rs::{email_update::GovUkChange, git::CommitBuilder};
    use lettre::{ClientSecurity, SmtpClient, Transport};
    use lettre_email::EmailBuilder;
    use std::{fs, net::TcpListener, path::Path};

    #[test]
    fn test_receive_updates() {
        let _ = std::fs::remove_dir_all("tests/tmp/inbox");
        let socket = TcpListener::bind("localhost:0").unwrap();
        let addr = socket.local_addr().unwrap();
        std::thread::spawn(move || {
            let (stream, remote_addr) = socket.accept().unwrap();
            receive_updates_on_socket(stream, remote_addr, "tests/tmp/inbox").unwrap();
        });

        let email = EmailBuilder::new()
            // Addresses can be specified by the tuple (email, alias)
            .to(("brexit@example.org", "Brexit"))
            // ... or by an address only
            .from("test@gov.uk")
            .subject("Hi, Hello world")
            .text(".Hello world.")
            .build()
            .unwrap();

        let mut mailer = SmtpClient::new(addr, ClientSecurity::None).unwrap().transport();
        mailer.send(email.into()).unwrap();
        assert_eq!(
            fs::read_dir("tests/tmp/inbox/gov.uk/brexit@example.org")
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn test_obtain_changes() -> Result<()> {
        const REPO_DIR: &str = "tests/tmp/test_obtain_changes";
        let _ = std::fs::remove_dir_all(REPO_DIR);
        let repo = Repository::init_bare(REPO_DIR)?;
        let test_sig = Signature::now("name", "email")?;
        CommitBuilder::new(&repo, None)?.commit(&test_sig, &test_sig, "initial commit")?;
        // let oid = repo.treebuilder(None)?.write()?;
        // let tree = repo.find_tree(oid)?;
        // repo.commit(Some(GIT_REF), &test_sig, &test_sig, "initial commit", &tree, &[])?;
        let commit = handle_change(
            &GovUkChange {
                url: "https://www.gov.uk/government/consultations/bus-services-act-2017-bus-open-data".parse()?,
                change: "testing the stuff".to_owned(),
                updated_at: "some time".to_owned(),
                category: Some("Test Category".to_owned()),
            },
            &repo,
            None,
        )?;
        repo.reference("refs/heads/main", commit.id(), false, "log_message")?;

        assert_eq!(commit.message(), Some("some time: testing the stuff [Test Category]"));
        assert_eq!(
            commit
                .tree()?
                .get_path(Path::new(
                    "government/consultations/bus-services-act-2017-bus-open-data.html"
                ))?
                .to_object(&repo)?
                .as_blob()
                .unwrap()
                .size(),
            20122
        );
        Ok(())
    }
}
