use anyhow::{bail, ensure, Context, Result};
use scraper::{html, ElementRef, Html, Selector};
use url::Url;

#[derive(PartialEq, Debug)]
pub struct GovUkChange {
    pub change: String,
    pub updated_at: String,
    pub url: Url,
    pub category: Option<String>,
}

impl GovUkChange {
    pub fn from_email_html(html: &str) -> Result<Vec<GovUkChange>> {
        let p = Selector::parse("p").unwrap();

        let html = Html::parse_document(html);
        let mut ps = html.select(&p);

        let email_title = {
            let p = ps.next().context("Missing first <p> with email subject")?;
            p.inner_html().trim_end().to_owned()
        };
        match email_title.as_ref() {
            "Update on GOV.\u{200B}UK." => parse_single(ps),
            "Update from GOV.\u{200b}UK for:" => parse_bulk(html),
            "Daily update from GOV.\u{200b}UK for:" => parse_bulk(html),
            "This link will stop working after 7 days."
            | "You’ll get an email from GOV.\u{200b}UK each time we add or update a page about:" => Ok(vec![]),
            title => bail!("Unexpected email title {:?}", title),
        }
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

    fn from_strs(change: String, href: &str, updated_at: String) -> Result<GovUkChange> {
        let mut url: Url = href.parse()?;
        ensure!(
            url.host_str() == Some("www.gov.uk"),
            "Unknown host : {:?}",
            url.host_str()
        );
        url.set_query(None);
        url.set_fragment(None);

        Ok(GovUkChange {
            change,
            url,
            updated_at,
            category: None,
        })
    }
}

fn parse_bulk(html: html::Html) -> Result<Vec<GovUkChange>> {
    let h2 = Selector::parse("h2").unwrap();
    let mut h2s = html.select(&h2);
    let category = {
        let h2 = h2s.next().context("Expected section heading")?;
        h2.inner_html()
    };
    let mut updates = vec![];
    for h2 in h2s {
        if let Some(mut update) = parse_bulk_update(h2).context("Something missing in part of a bulk update")? {
            update.category = Some(category.clone());
            updates.push(update);
        }
    }
    Ok(updates)
}

fn parse_bulk_update(h2: ElementRef) -> Result<Option<GovUkChange>> {
    let (_doc_title, href) = {
        let child = h2.first_child().context("update heading missing link")?;
        if child.value().as_text().map(|t| &**t) == Some("Why am I getting this email?") {
            return Ok(None);
        }
        let a = ElementRef::wrap(child).context(format!("expected <a> elem, found {:?}", child.value()))?;
        ensure!(a.value().name() == "a");
        (a.inner_html(), a.value().attr("href").context("missing href")?)
    };
    let mut siblings = h2.next_siblings().map(ElementRef::wrap).flatten();
    let (change, updated_at) = parse_common(&mut siblings)?;
    ensure!(siblings.next().map(|e| e.value().name()) == Some("hr"));

    Ok(Some(GovUkChange::from_strs(change, href, updated_at)?))
}

fn parse_single(mut ps: html::Select) -> Result<Vec<GovUkChange>> {
    let (_doc_title, href) = {
        let p = ps.next().context("Missing second <p> with doc title")?;
        let doc_link_elem = ElementRef::wrap(p.first_child().context("Empty doc title <p>")?).unwrap();
        let doc_title = doc_link_elem.inner_html();
        let href = doc_link_elem.value().attr("href").context("No link on doc title")?;
        (doc_title, href)
    };
    let (change, updated_at) = parse_common(&mut ps)?;

    Ok(vec![GovUkChange::from_strs(change, href, updated_at)?])
}

fn parse_common<'a>(ps: &mut impl Iterator<Item = ElementRef<'a>>) -> Result<(String, String)> {
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
    Ok((change, updated_at))
}

#[test]
fn test_single_email_parse() {
    let updates = GovUkChange::from_eml(include_str!("../tests/emails/GOV.UK single update.eml")).unwrap();
    assert_eq!(
        updates,
        vec![GovUkChange {
            change: "Updated Germany Doctors List – December 2020".to_owned(),
            updated_at: "12:13pm, 9 December 2020".to_owned(),
            url: "https://www.gov.uk/government/publications/germany-list-of-medical-practitionersfacilities"
                .parse()
                .unwrap(),
            category: None,
        }]
    )
}

#[test]
fn test_single_2021_email_parse() {
    let updates = GovUkChange::from_eml(include_str!("../tests/emails/GOV.UK single update 2021.eml")).unwrap();
    assert_eq!(
        updates,
        vec![GovUkChange {
            change: "First published.".to_owned(),
            updated_at: "10:29am, 23 January 2021".to_owned(),
            url: "https://www.gov.uk/government/news/uk-to-host-g7-summit-in-cornwall"
                .parse()
                .unwrap(),
            category: Some("News and communications".to_owned()),
        }]
    )
}

#[test]
fn test_daily_email_parse() {
    let updates = GovUkChange::from_eml(include_str!("../tests/emails/GOV.UK daily update.eml")).unwrap();
    assert_eq!(updates.len(), 60);
    assert_eq!(
        GovUkChange {
            change: "Under ‘What care homes and other social care settings must do during an outbreak’ and ‘Repeat testing’, updated the length of time that staff or residents who have been diagnosed with COVID-19 should not be included in testing – to 90 days after either their initial onset of symptoms or their positive test result (if they were asymptomatic when tested).".to_owned(),
            updated_at: "8:06am, 22 January 2021".to_owned(),
            url: "https://www.gov.uk/guidance/overview-of-adult-social-care-guidance-on-coronavirus-covid-19".parse().unwrap(),
            category: Some("Coronavirus (COVID-19)".to_owned()),
        },
        updates[0]
    );
    for update in &updates {
        assert_eq!(update.url.host_str(), Some("www.gov.uk"));
    }
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
            category: None,
        }]
    )
}
