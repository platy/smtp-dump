use anyhow::{bail, Context, Result};
use async_io::{block_on, Async, Timer};
use chrono::{SecondsFormat, Utc};
use file_lock::FileLock;
use futures_lite::{io::BufReader, AsyncBufReadExt, FutureExt};
use std::{
    fs,
    io::{self, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

struct MailHandler {
    peer_addr: SocketAddr,
    inbox: PathBuf,
    data: Option<EmailWrite>,
}

macro_rules! maillog {
    ($peer_addr:expr, $($arg:tt)*) => {
        println!("{} [{}]: {}", Utc::now(), $peer_addr, format_args!($($arg)*));
    };
}

impl mailin::Handler for MailHandler {
    fn helo(&mut self, _ip: std::net::IpAddr, _domain: &str) -> mailin::Response {
        // maillog!(self.peer_addr, "HELO {} {}", ip, domain);
        mailin::response::OK
    }

    fn mail(&mut self, _ip: std::net::IpAddr, _domain: &str, from: &str) -> mailin::Response {
        maillog!(self.peer_addr, "MAIL {}", from);
        mailin::response::OK
    }

    fn rcpt(&mut self, to: &str) -> mailin::Response {
        maillog!(self.peer_addr, "RCPT {}", to);
        mailin::response::OK
    }

    fn data_start(&mut self, _domain: &str, from: &str, _is8bit: bool, to: &[String]) -> mailin::Response {
        let email_path = inbox_path_for_email(&self.inbox, from, to);
        match EmailWrite::create(email_path) {
            Ok(writer) => {
                println!(
                    "{}: Writing email to {} (via tmp)",
                    self.peer_addr,
                    writer.path.to_str().unwrap_or_default()
                );
                self.data = Some(writer);
                mailin::response::OK
            }
            Err(err) => {
                maillog!(self.peer_addr, "Error mapping email envelope to inbox : {}", err);
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
        if let Some(writer) = self.data.take() {
            match writer.end() {
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
    temp_file: temp_file::TempFile,
    lock: FileLock,
}

impl EmailWrite {
    fn create(path: PathBuf) -> Result<Self> {
        fs::create_dir_all(path.parent().unwrap())?;
        let temp_file = temp_file::empty();
        Ok(EmailWrite {
            lock: FileLock::lock(temp_file.path().to_str().unwrap(), true, true)?,
            temp_file,
            path,
        })
    }

    fn end(mut self) -> std::io::Result<()> {
        self.flush()?;
        fs::rename(self.temp_file.path(), &self.path)
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

fn inbox_path_for_email(inbox: &Path, from: &str, to: &[String]) -> PathBuf {
    let from_domain = from.split('@').nth(1);
    inbox
        .join(from_domain.unwrap_or(from))
        .join(to.join(","))
        .join(Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true))
        .with_extension("eml")
}

/// accepts emails and saves them in `inbox/{from}/{to}/{datetime}.eml
fn receive_updates_on_socket(mut stream: TcpStream, inbox: impl AsRef<Path>) -> Result<()> {
    let peer_addr = stream.peer_addr()?;
    let remote_addr = stream.peer_addr().unwrap();
    let handler = MailHandler {
        peer_addr,
        inbox: inbox.as_ref().to_path_buf(),
        data: None,
    };
    let mut session = mailin::SessionBuilder::new("gitgov").build(remote_addr.ip(), handler);
    session.greeting().write_to(&mut stream)?;

    let mut buf_read = BufReader::new(Async::new(stream.try_clone()?)?);
    let mut command = String::new();

    loop {
        command.clear();
        let len = block_on(buf_read.read_line(&mut command).or(async {
            Timer::after(Duration::from_secs(5 * 60)).await;
            Err(std::io::ErrorKind::TimedOut.into())
        }))?;
        let command = if len == 0 {
            break;
        } else {
            &command[..]
        };
        let result = session.process(command.as_bytes());
        match result.action {
            mailin::Action::Close => {
                maillog!(peer_addr, "CLOSE");
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
    let socket = TcpListener::bind("0.0.0.0:25")?;
    let inbox_dir_opt = std::env::var("INBOX_DIR");
    let inbox_dir = inbox_dir_opt.as_deref().unwrap_or("inbox");
    socket.incoming().for_each(|res| match res {
        Ok(conn) => {
            if let Err(err) = receive_updates_on_socket(conn, inbox_dir) {
                println!("Closed SMTP session due to error : {}", err);
            }
        }
        Err(err) => eprintln!("Failure accepting connection :{}", err),
    });
    Ok(())
}

#[cfg(test)]
mod test {
    use super::receive_updates_on_socket;
    use lettre::{ClientSecurity, SmtpClient, Transport};
    use lettre_email::EmailBuilder;
    use std::{fs, net::TcpListener};

    #[test]
    fn test_receive_updates() {
        let _ = std::fs::remove_dir_all("tests/tmp/inbox");
        let socket = TcpListener::bind("localhost:0").unwrap();
        let addr = socket.local_addr().unwrap();
        std::thread::spawn(move || {
            let (stream, _) = socket.accept().unwrap();
            receive_updates_on_socket(stream, "tests/tmp/inbox").unwrap();
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
}
