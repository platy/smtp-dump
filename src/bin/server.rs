use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use dotenv::dotenv;
use git2::{Commit, Repository, Signature};
use gitgov_rs::{email_update::GovUkChange, git::CommitBuilder, retrieve_doc};
use mailin::{Handler, MailResult, SessionBuilder};
use std::{
    collections::VecDeque,
    fs::{create_dir_all, read, read_dir, rename, DirEntry, File},
    io::{BufRead, BufReader},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    thread,
    thread::yield_now,
};
use url::Url;

#[derive(Clone)]
struct MailHandler {
    peer_addr: SocketAddr,
    inbox: PathBuf,
}

impl Handler for MailHandler {
    fn helo(&mut self, ip: std::net::IpAddr, domain: &str) -> mailin::HeloResult {
        println!("{}: HELO {} {}", self.peer_addr, ip, domain);
        mailin::HeloResult::Ok
    }

    fn mail(&mut self, ip: std::net::IpAddr, domain: &str, from: &str) -> mailin::MailResult {
        println!("{}: MAIL {}", self.peer_addr, from);
        let from_match = dotenv::var("FROM_FILTER")
            .ok()
            .map(|from_filter| from.contains(&from_filter));
        if from_match == Some(false) {
            println!(
                "{}: {}({}) tried to send some junk, purporting to be {}",
                self.peer_addr, domain, ip, from
            );
            MailResult::NoService
        } else {
            MailResult::Ok
        }
    }

    fn rcpt(&mut self, to: &str) -> mailin::RcptResult {
        println!("{}: RCPT {}", self.peer_addr, to);
        mailin::RcptResult::Ok
    }

    fn data(&mut self, _domain: &str, from: &str, _is8bit: bool, to: &[String]) -> mailin::DataResult {
        let email_path = inbox_path_for_email(&self.inbox, from, to);
        match create_dir_all(email_path.parent().unwrap()).and_then(|_| File::create(&email_path)) {
            Ok(file) => {
                println!(
                    "{}: Writing email to {}",
                    self.peer_addr,
                    email_path.to_str().unwrap_or_default()
                );
                mailin::DataResult::Ok(Box::new(file))
            }
            Err(err) => {
                println!("{}: Error mapping email envelope to inbox : {}", self.peer_addr, err);
                mailin::DataResult::InternalError
            }
        }
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
    };
    let mut session = SessionBuilder::new("gitgov").build(remote_addr.ip(), handler);
    session.greeting().write_to(&mut stream)?;

    let mut buf_read = BufReader::new(stream.try_clone()?);
    let mut command = String::new();

    loop {
        command.clear();
        let len = buf_read.read_line(&mut command)?;
        let command = if len == 0 {
            break;
        } else if command.starts_with('.') && command != ".\r\n" {
            // undo dot stuffing
            println!("Undoing dot stuffing on line {:?}", command);
            &command[1..]
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
    create_dir_all(EMAILS_FROM_GOVUK_PATH).context(format!("Error trying to create dir {}", EMAILS_FROM_GOVUK_PATH))?;
    create_dir_all(ARCHIVE_DIR).context(format!("Error trying to create dir {}", ARCHIVE_DIR))?;

    if dotenv::var("DISABLE_PROCESS_UPDATES").is_err() {
        thread::spawn(move || {
            loop {
                process_updates_in_dir(EMAILS_FROM_GOVUK_PATH, ARCHIVE_DIR, &repo_path, &reference)
                    .expect("the processing fails, the repo may be unclean");
                yield_now();
            }
        });
    }

    let socket = TcpListener::bind("0.0.0.0:25")?;
    loop {
        let (stream, remote_addr) = socket.accept()?;
        if let Err(err) = receive_updates_on_socket(stream, remote_addr, "inbox") {
            println!("Closed SMTP session due to error : {}", err);
        }
    }
}

fn process_updates_in_dir(
    in_dir: impl AsRef<Path>,
    out_dir: impl AsRef<Path>,
    repo: impl AsRef<Path>,
    reference: &str,
) -> Result<()> {
    for to_inbox in read_dir(in_dir)? {
        let to_inbox = to_inbox?;
        if to_inbox.metadata()?.is_dir() {
            for email in read_dir(to_inbox.path())? {
                let email = email?;
                process_email_update_file(to_inbox.file_name(), &email, &out_dir, &repo, reference).context(
                    format!("Failed processing {}", email.path().to_str().unwrap_or_default()),
                )?;
            }
        }
    }
    Ok(())
}

fn process_email_update_file(
    to_dir_name: impl AsRef<Path>,
    dir_entry: &DirEntry,
    out_dir: impl AsRef<Path>,
    repo_base: impl AsRef<Path>,
    reference: &str,
) -> Result<()> {
    let data = read(dir_entry.path()).context("Reading email file")?;
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
    create_dir_all(done_path.parent().unwrap()).context("Creating outbox dir")?;
    rename(dir_entry.path(), &done_path).context(format!(
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

    let message = format!("{}: {} [Category]", updated_at, change);
    let govuk_sig = Signature::now("Gov.uk", "info@gov.uk")?;
    let gitgov_sig = Signature::now("Gitgov", "gitgov@njk.onl")?;
    Ok(commit_builder.commit(&govuk_sig, &gitgov_sig, &message)?)
}

fn fetch_change(url: &Url, mut write_out: impl FnMut(PathBuf, &[u8]) -> Result<()>) -> Result<()> {
    let mut urls = VecDeque::new();
    urls.push_back(url.to_owned());

    while let Some(url) = urls.pop_front() {
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
    use std::{net::TcpListener, path::Path};

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
            std::fs::read_dir("tests/tmp/inbox/gov.uk/brexit@example.org")
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn test_obtain_changes() -> Result<()> {
        const REPO_DIR: &str = "tests/tmp/repo";
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
            },
            &repo,
            None,
        )?;

        assert_eq!(commit.message(), Some("some time: testing the stuff [Category]"));
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
        assert_eq!(commit.tree()?.get_path(Path::new("government/uploads/system/uploads/attachment_data/file/792313/bus-open-data-consultation-response.pdf"))?.to_object(&repo)?.as_blob().unwrap().size(), 643743);
        Ok(())
    }
}
