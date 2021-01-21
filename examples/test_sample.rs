//! Send a sample email to the locally running server to test it out

use anyhow::Result;
use lettre::{ClientSecurity, SmtpClient, Transport};
use lettre_email::EmailBuilder;

fn main() -> Result<()> {
    let email = EmailBuilder::new()
        .to(("brexit@example.org", "Brexit"))
        .from("test@gov.uk")
        .subject("Hi, Hello world")
        .html(include_str!("../tests/emails/new-email-format.html"))
        .build()
        .unwrap();

    let mut mailer = SmtpClient::new("localhost:22122", ClientSecurity::None)
        .unwrap()
        .transport();
    mailer.send(email.into()).unwrap();
    Ok(())
}
