use std::{io::{BufRead, BufReader, Write}, net::TcpListener, sync::mpsc::{SyncSender, sync_channel}, thread};
use gitgov_rs::email_update::GovUkChange;
use mailin::{Handler, MailResult, SessionBuilder};

#[derive(Clone)]
struct MailHandler {
    sender: SyncSender<(Vec<u8>, Vec<String>)>, // email body and to addresses
    current: Option<(Vec<u8>, Vec<String>)>,
}

impl MailHandler {
    fn send_current(&mut self) {
        if let Some(message) = self.current.take() {
            if let Err(err) = self.sender.send(message) {
                println!("Sender error : {}", err);
            }
        }
    }
}

impl Handler for MailHandler {
    fn helo(&mut self, _ip: std::net::IpAddr, _domain: &str) -> mailin_embedded::HeloResult {
        mailin_embedded::HeloResult::Ok
    }

    fn mail(&mut self, ip: std::net::IpAddr, domain: &str, from: &str) -> mailin_embedded::MailResult {
        if !from.contains("gov.uk") {
            println!("{} tried to send some junk to {}, purporting to be {}", ip, domain, from);
            MailResult::NoService
        } else {
            MailResult::Ok
        }
    }

    fn rcpt(&mut self, _to: &str) -> mailin_embedded::RcptResult {
        mailin_embedded::RcptResult::Ok
    }

    fn data(&mut self, _domain: &str, _from: &str, _is8bit: bool, to: &[String]) -> mailin_embedded::DataResult {
        self.send_current();
        let mut current_email = self.current.get_or_insert((vec![], to.into()));
        mailin_embedded::DataResult::Ok(Box::new(&mut current_email.0))
    }
}

impl Drop for MailHandler {
    fn drop(&mut self) {
        self.send_current();
    }
}

fn main() {
    let (sender, receiver) = sync_channel(1);

    thread::spawn(move || {
        while let Ok((data, to)) = receiver.recv() {
            handle_email(data, to);
        }
    });

    let socket = TcpListener::bind("localhost:22122").unwrap();
    loop {
        let (mut stream, remote_addr) = socket.accept().unwrap();
        let handler = MailHandler {
            sender: sender.clone(),
            current: None,
        };
        let mut session = SessionBuilder::new("gitgov").build(remote_addr.ip(), handler);
        session.greeting().write_to(&mut stream).unwrap();
        
        let mut buf_read = BufReader::new(stream.try_clone().unwrap());
        let mut buf = String::new();

        loop {
            let len = buf_read.read_line(&mut buf).unwrap();
            let result = session.process(&buf.as_bytes()[..len]);
            match result.action {
                mailin::Action::Close => {
                    result.write_to(&mut stream).unwrap();
                    break
                }
                mailin::Action::UpgradeTls => panic!("TLS requested"),
                mailin::Action::NoReply => continue,
                mailin::Action::Reply => result.write_to(&mut stream).unwrap(),
            }
        }
    }
}

fn handle_email(email_data: Vec<u8>, to: Vec<String>) {
    let updates = GovUkChange::from_eml(&String::from_utf8(email_data).unwrap()).unwrap();

}
