use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use dotenv::dotenv;
use git2::{Repository, Signature};
use gitgov_rs::{email_update::GovUkChange, git::CommitBuilder, retrieve_doc};
use mailin::{Handler, MailResult, SessionBuilder};
use std::{
    collections::VecDeque,
    fs::{create_dir_all, read, read_dir, remove_file, File},
    io::{BufRead, BufReader},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    thread,
    thread::yield_now,
};
use url::Url;

#[derive(Clone)]
struct MailHandler {
    inbox: PathBuf,
}

impl Handler for MailHandler {
    fn helo(&mut self, _ip: std::net::IpAddr, _domain: &str) -> mailin::HeloResult {
        mailin::HeloResult::Ok
    }

    fn mail(&mut self, ip: std::net::IpAddr, domain: &str, from: &str) -> mailin::MailResult {
        if !from.contains("gov.uk") {
            println!("{}({}) tried to send some junk, purporting to be {}", domain, ip, from);
            MailResult::NoService
        } else {
            MailResult::Ok
        }
    }

    fn rcpt(&mut self, _to: &str) -> mailin::RcptResult {
        mailin::RcptResult::Ok
    }

    fn data(&mut self, _domain: &str, from: &str, _is8bit: bool, to: &[String]) -> mailin::DataResult {
        let file_path = self
            .inbox
            .join(from)
            .join(to.join(","))
            .join(Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true))
            .with_extension("eml");
        create_dir_all(file_path.parent().unwrap()).unwrap();
        match File::create(file_path) {
            Ok(file) => mailin::DataResult::Ok(Box::new(file)),
            Err(err) => {
                println!("Error creating email file to write : {}", err);
                mailin::DataResult::NoService
            }
        }
    }
}

fn main() -> Result<()> {
    dotenv()?;
    const EMAILS_FROM_GOVUK_PATH: &str = "inbox/test@gov.uk";
    let repo_path = std::env::var("REPO")?;
    let reference = std::env::var("REF")?;
    create_dir_all(EMAILS_FROM_GOVUK_PATH)?;
    thread::spawn(move || {
        loop {
            process_updates_in_dir(EMAILS_FROM_GOVUK_PATH, &repo_path, &reference).unwrap(); // if the processing fails, the repo may be unclean
            yield_now();
        }
    });

    let socket = TcpListener::bind("localhost:22122")?;
    loop {
        let (stream, remote_addr) = socket.accept()?;
        receive_updates_on_socket(stream, remote_addr, "inbox");
    }
}

fn process_updates_in_dir(dir: impl AsRef<Path>, repo: impl AsRef<Path>, reference: &str) -> Result<()> {
    for to_inbox in read_dir(dir)? {
        let to_inbox = to_inbox?;
        if to_inbox.metadata()?.is_dir() {
            for email in read_dir(to_inbox.path())? {
                let email = email?;
                let data = read(email.path())?;
                let updates = GovUkChange::from_eml(&String::from_utf8(data)?)?;
                for GovUkChange { url, change, .. } in updates {
                    handle_change(url, &repo, &change, reference)?;
                }
                // successfully handled, delete
                remove_file(email.path())?;
            }
        }
    }
    Ok(())
}

/// accepts emails from gov.uk and saves them in `inbox/{from}/{to}/{datetime}.eml
fn receive_updates_on_socket(mut stream: TcpStream, remote_addr: SocketAddr, inbox: impl AsRef<Path>) {
    let handler = MailHandler {
        inbox: inbox.as_ref().to_path_buf(),
    };
    let mut session = SessionBuilder::new("gitgov").build(remote_addr.ip(), handler);
    session.greeting().write_to(&mut stream).unwrap();

    let mut buf_read = BufReader::new(stream.try_clone().unwrap());
    let mut buf = String::new();

    loop {
        buf.clear();
        let len = buf_read.read_line(&mut buf).unwrap();
        let result = session.process(&buf.as_bytes()[..len]);
        match result.action {
            mailin::Action::Close => {
                result.write_to(&mut stream).unwrap();
                break;
            }
            mailin::Action::UpgradeTls => panic!("TLS requested"),
            mailin::Action::NoReply => continue,
            mailin::Action::Reply => match result.write_to(&mut stream) {
                Ok(()) => {}
                Err(err) => {
                    println!("Writing SMTP reply failed : {}", &err);
                    break;
                }
            },
        }
    }
}

fn handle_change(url: Url, repo_base: impl AsRef<Path>, message: &str, reference: &str) -> Result<()> {
    let repo = Repository::open(repo_base)?;
    let mut commit_builder = CommitBuilder::new(&repo, reference)?;

    fetch_change(url, |path, bytes| {
        // write the blob
        let oid = repo.blob(bytes)?;
        commit_builder.add_to_tree(path.to_str().unwrap(), oid, 0o100644)
    })?;

    let govuk_sig = Signature::now("Gov.uk", "info@gov.uk")?;
    let gitgov_sig = Signature::now("Gitgov", "gitgov@njk.onl")?;
    commit_builder.commit(&govuk_sig, &gitgov_sig, message)?;

    Ok(())
}

fn fetch_change(url: Url, mut write_out: impl FnMut(PathBuf, &[u8]) -> Result<()>) -> Result<()> {
    let mut urls = VecDeque::new();
    urls.push_back(url);

    while let Some(url) = urls.pop_front() {
        let doc = retrieve_doc(url)?;
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
    use gitgov_rs::git::CommitBuilder;
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
            receive_updates_on_socket(stream, remote_addr, "tests/tmp/inbox");
        });

        let email = EmailBuilder::new()
            // Addresses can be specified by the tuple (email, alias)
            .to(("brexit@example.org", "Brexit"))
            // ... or by an address only
            .from("test@gov.uk")
            .subject("Hi, Hello world")
            .text("Hello world.")
            .build()
            .unwrap();

        let mut mailer = SmtpClient::new(addr, ClientSecurity::None).unwrap().transport();
        mailer.send(email.into()).unwrap();
        assert_eq!(
            std::fs::read_dir("tests/tmp/inbox/test@gov.uk/brexit@example.org")
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn test_obtain_changes() -> Result<()> {
        const REPO_DIR: &str = "tests/tmp/repo";
        const GIT_REF: &str = "refs/heads/main";
        let _ = std::fs::remove_dir_all(REPO_DIR);
        let repo = Repository::init_bare(REPO_DIR)?;
        let test_sig = Signature::now("name", "email")?;
        CommitBuilder::new(&repo, GIT_REF)?.commit(&test_sig, &test_sig, "initial commit")?;
        // let oid = repo.treebuilder(None)?.write()?;
        // let tree = repo.find_tree(oid)?;
        // repo.commit(Some(GIT_REF), &test_sig, &test_sig, "initial commit", &tree, &[])?;
        handle_change(
            "https://www.gov.uk/government/consultations/bus-services-act-2017-bus-open-data".parse()?,
            REPO_DIR,
            "testing the stuff",
            GIT_REF,
        )?;

        let head_commit = repo.find_reference(GIT_REF)?.peel_to_commit()?;
        assert_eq!(head_commit.message(), Some("testing the stuff"));
        assert_eq!(
            head_commit
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
        assert_eq!(head_commit.tree()?.get_path(Path::new("government/uploads/system/uploads/attachment_data/file/792313/bus-open-data-consultation-response.pdf"))?.to_object(&repo)?.as_blob().unwrap().size(), 643743);
        Ok(())
    }
}
