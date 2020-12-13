use scraper::{ElementRef, Html, Selector};
use url::Url;

#[derive(PartialEq, Debug)]
pub struct GovUkChange {
    change: String,
    updated_at: String,
    url: Url,
}

impl GovUkChange {
    pub fn from_email_html(html: &str) -> Result<Vec<GovUkChange>, &'static str> {
        println!("html {}", html);
        let html = Html::parse_document(html);

        let p = Selector::parse("p").unwrap();
        let mut iter = html.select(&p);
        assert_eq!(iter.next().unwrap().inner_html().trim_end(), "Update on GOV.​UK.");
        let doc_link_elem = ElementRef::wrap(iter.next().unwrap().first_child().unwrap()).unwrap();
        let mut url: Url = doc_link_elem.value().attr("href").unwrap().parse().unwrap();
        let _doc_title = doc_link_elem.inner_html();
        let _page_summary = iter.next().unwrap().inner_html();
        let change = iter.next().unwrap().text().skip(1).next().unwrap().to_owned();
        let updated_at = iter.next().unwrap().text().skip(1).next().unwrap().to_owned();

        url.set_query(None);
        url.set_fragment(None);

        Ok(vec![
            GovUkChange {
                change,
                updated_at,
                url,
            }
        ])
    }

    pub fn from_eml(eml: &str) -> Result<Vec<GovUkChange>, &'static str> {
        let email = mailparse::parse_mail(eml.as_bytes()).map_err(|_| "failed to parse email")?;
        let body = email.subparts[1].get_body().map_err(|_| "failed to parse email body")?;
        GovUkChange::from_email_html(&body)
    }
}

#[test]
fn test_email_parse() {
    let updates = GovUkChange::from_eml(include_str!("../tests/emails/GOV.UK single update.eml")).unwrap();
    assert_eq!(
        updates,
        vec![
            GovUkChange {
                change: "Updated Germany Doctors List – December 2020".to_owned(),
                updated_at: "12:13pm, 9 December 2020".to_owned(),
                url: "https://www.gov.uk/government/publications/germany-list-of-medical-practitionersfacilities".parse().unwrap(),
            }
        ]
    )
}

#[test]
fn test_html_parse() {
    let updates =
        GovUkChange::from_email_html(include_str!("../tests/emails/new-email-format.html"))
            .unwrap();
    assert_eq!(
        updates,
        vec![GovUkChange {
            change: "Forms EC3163 and EC3164 updated".to_owned(),
            updated_at: "10:35am, 10 July 2019".to_owned(),
            url: "https://www.gov.uk/guidance/export-live-animals-special-rules".parse().unwrap(),
        }]
    )
}
