use chrono::{SecondsFormat, Utc};
use gitgov_rs::email_update::GovUkChange;
use gitgov_rs::retrieve_doc;
use mailin::{Handler, MailResult, SessionBuilder};
use std::collections::VecDeque;
use std::fs::create_dir_all;
use std::fs::read;
use std::fs::read_dir;
use std::fs::remove_file;
use std::fs::File;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::thread::yield_now;
use std::{
    io::{BufRead, BufReader},
    net::TcpListener,
    thread,
};

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
            println!(
                "{}({}) tried to send some junk, purporting to be {}",
                domain, ip, from
            );
            MailResult::NoService
        } else {
            MailResult::Ok
        }
    }

    fn rcpt(&mut self, _to: &str) -> mailin::RcptResult {
        mailin::RcptResult::Ok
    }

    fn data(
        &mut self,
        _domain: &str,
        from: &str,
        _is8bit: bool,
        to: &[String],
    ) -> mailin::DataResult {
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

fn main() {
    const EMAILS_FROM_GOVUK_PATH: &str = "inbox/gitgov.njk.onl/info@gov.uk";
    create_dir_all(EMAILS_FROM_GOVUK_PATH).unwrap();
    thread::spawn(move || loop {
        process_updates_in_dir(EMAILS_FROM_GOVUK_PATH);
        yield_now();
    });

    let socket = TcpListener::bind("localhost:22122").unwrap();
    loop {
        let (stream, remote_addr) = socket.accept().unwrap();
        receive_updates_on_socket(stream, remote_addr, "inbox");
    }
}

fn process_updates_in_dir(dir: impl AsRef<Path>) {
    for to_inbox in read_dir(dir).unwrap() {
        let to_inbox = to_inbox.unwrap();
        if to_inbox.metadata().unwrap().is_dir() {
            for email in read_dir(to_inbox.path()).unwrap() {
                let email = email.unwrap();
                let data = read(email.path()).unwrap();
                handle_email(data);
                // successfully handled, delete
                remove_file(email.path()).unwrap();
            }
        }
    }
}

/// accepts emails from gov.uk and saves them in `inbox/{from}/{to}/{datetime}.eml
fn receive_updates_on_socket(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    inbox: impl AsRef<Path>,
) {
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

fn handle_email(email_data: Vec<u8>) {
    let repo_base: PathBuf = "repo".into();

    let updates = GovUkChange::from_eml(&String::from_utf8(email_data).unwrap()).unwrap();
    for GovUkChange { url, .. } in updates {
        let mut urls = VecDeque::new();
        urls.push_back(url);

        while let Some(url) = urls.pop_front() {
            let doc = retrieve_doc(url).unwrap();
            urls.extend(
                doc.content
                    .attachments()
                    .unwrap_or_default()
                    .iter()
                    .cloned(),
            );

            let mut path = repo_base.join(doc.url.path().strip_prefix("/").unwrap());
            if doc.content.is_html() {
                assert!(path.set_extension("html"));
            }
            let _ = create_dir_all(path.parent().unwrap());
            println!("Writing doc to : {}", path.to_str().unwrap());
            let mut file = File::create(path).unwrap();
            file.write_all(doc.content.as_bytes()).unwrap();
        }
    }
}

#[cfg(test)]
mod test {
    use super::receive_updates_on_socket;
    use lettre::SmtpClient;
    use lettre::{ClientSecurity, Transport};
    use lettre_email::EmailBuilder;
    use std::net::TcpListener;

    #[test]
    fn test_receive_updates() {
        std::fs::remove_dir_all("tests/inbox").unwrap();
        let socket = TcpListener::bind("localhost:0").unwrap();
        let addr = socket.local_addr().unwrap();
        std::thread::spawn(move || {
            let (stream, remote_addr) = socket.accept().unwrap();
            receive_updates_on_socket(stream, remote_addr, "tests/inbox");
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

        let mut mailer = SmtpClient::new(addr, ClientSecurity::None)
            .unwrap()
            .transport();
        mailer.send(email.into()).unwrap();
        assert_eq!(
            std::fs::read_dir("tests/inbox/test@gov.uk/brexit@example.org")
                .unwrap()
                .count(),
            1
        );
    }
}
