use anyhow::{Context, Result};
use scraper::{ElementRef, Html, Selector};
use url::Url;

#[derive(PartialEq, Debug)]
pub struct GovUkChange {
    pub change: String,
    pub updated_at: String,
    pub url: Url,
}

impl GovUkChange {
    pub fn from_email_html(html: &str) -> Result<Vec<GovUkChange>> {
        let p = Selector::parse("p").unwrap();

        let html = Html::parse_document(html);
        let mut ps = html.select(&p);

        {
            let p = ps.next().context("Missing first <p> with email subject")?;
            assert_eq!(p.inner_html().trim_end(), "Update on GOV.\u{200B}UK.");
        }
        let mut url: Url = {
            let p = ps.next().context("Missing second <p> with doc title")?;
            let doc_link_elem = ElementRef::wrap(p.first_child().context("Empty doc title <p>")?).unwrap();
            let _doc_title = doc_link_elem.inner_html();
            doc_link_elem
                .value()
                .attr("href")
                .context("No link on doc title")?
                .parse()?
        };
        let _page_summary = {
            let p = ps.next().context("Missing third <p> with doc summary")?;
            p.inner_html()
        };
        let change = {
            let p = ps.next().context("Missing forth <p> with change description")?;
            p.text()
                .nth(1)
                .context("Missing change description contents")?
                .to_owned()
        };
        let updated_at = {
            let p = ps.next().context("Missing fifth <p> with updated timestamp")?;
            p.text().nth(1).context("Missing timestamp <p> contents")?.to_owned()
        };

        url.set_query(None);
        url.set_fragment(None);

        Ok(vec![GovUkChange {
            change,
            updated_at,
            url,
        }])
    }

    pub fn from_eml(eml: &str) -> Result<Vec<GovUkChange>> {
        let email = mailparse::parse_mail(eml.as_bytes()).context("failed to parse email")?;
        let part = email
            .subparts
            .into_iter()
            .find(|part| part.ctype.mimetype == "text/html")
            .context("Email doesn't have text/html part")?;
        let body = part.get_body().context("failed to parse email body")?;
        GovUkChange::from_email_html(&body)
    }
}

#[test]
fn test_email_parse() {
    let updates = GovUkChange::from_eml(include_str!("../tests/emails/GOV.UK single update.eml")).unwrap();
    assert_eq!(
        updates,
        vec![GovUkChange {
            change: "Updated Germany Doctors List – December 2020".to_owned(),
            updated_at: "12:13pm, 9 December 2020".to_owned(),
            url: "https://www.gov.uk/government/publications/germany-list-of-medical-practitionersfacilities"
                .parse()
                .unwrap(),
        }]
    )
}

#[test]
fn test_html_parse() {
    let updates = GovUkChange::from_email_html(include_str!("../tests/emails/new-email-format.html")).unwrap();
    assert_eq!(
        updates,
        vec![GovUkChange {
            change: "Forms EC3163 and EC3164 updated".to_owned(),
            updated_at: "10:35am, 10 July 2019".to_owned(),
            url: "https://www.gov.uk/guidance/export-live-animals-special-rules"
                .parse()
                .unwrap(),
        }]
    )
}
