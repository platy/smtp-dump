use anyhow::{format_err, Context, Result};
use chrono::{DateTime, Utc};
use lol_html::{element, rewrite_str, RewriteStrSettings};
use scraper::{Html, Selector};
use url::Url;

#[derive(Debug, Eq, PartialEq)]
pub struct Doc {
    pub url: Url,
    pub content: DocContent,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DocContent {
    DiffableHtml(String, Vec<Url>, Vec<DocUpdate>),
    Other(Vec<u8>),
}

#[derive(Debug, Eq, PartialEq)]
pub struct DocUpdate(DateTime<Utc>, String);

impl DocContent {
    pub fn html(html: &str, url: Option<&Url>) -> Result<Self> {
        let main_selector: Selector = Selector::parse("main").unwrap();
        let history_selector: Selector = Selector::parse("#full-history li").unwrap();
        let time_selector: Selector = Selector::parse("time").unwrap();
        let p_selector: Selector = Selector::parse("p").unwrap();

        let html = Html::parse_document(html);

        let main = html.select(&main_selector).next().context("No main found")?;
        let mut history = vec![];
        for history_elem in html.select(&history_selector) {
            let time_elem = history_elem.select(&time_selector).next().context("No time found")?;
            history.push(DocUpdate(
                time_elem
                    .value()
                    .attr("datetime")
                    .context("Missing \"datetime\" property on time tag")?
                    .parse()?,
                history_elem
                    .select(&p_selector)
                    .next()
                    .context("Missing p tag")?
                    .inner_html(),
            ))
        }
        let mut attachments = vec![];
        for attachment_url in attachments_from(&html) {
            attachments.push(if let Some(url) = url {
                url.join(&attachment_url)?
            } else {
                attachment_url.parse()?
            });
        }
        Ok(DocContent::DiffableHtml(
            remove_ids(&main.html())?,
            attachments,
            history,
        ))
    }

    pub fn is_html(&self) -> bool {
        match self {
            Self::DiffableHtml(_, _, _) => true,
            Self::Other(_) => false,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            DocContent::DiffableHtml(string, _, _) => string.as_bytes(),
            DocContent::Other(bytes) => bytes.as_slice(),
        }
    }

    pub fn history(&self) -> Option<&[DocUpdate]> {
        match self {
            DocContent::DiffableHtml(_, _, history) => Some(history.as_slice()),
            DocContent::Other(_) => None,
        }
    }

    pub fn attachments(&self) -> Option<&[Url]> {
        match self {
            DocContent::DiffableHtml(_, attachments, _) => Some(attachments.as_slice()),
            DocContent::Other(_) => None,
        }
    }
}

impl DocUpdate {
    pub fn new(date: DateTime<Utc>, summary: impl Into<String>) -> Self {
        Self(date, summary.into())
    }
}

pub fn remove_ids(html: &str) -> Result<String> {
    rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("[id]", |el| {
                    // dynamically generated ids
                    el.remove_attribute("id");
                    Ok(())
                }),
                element!("[aria-labelledby]", |el| {
                    // dynamicaly generated ids
                    el.remove_attribute("aria-labelledby");
                    Ok(())
                }),
                element!("[aria-hidden]", |el| {
                    // i don't really want to strip out aria stuff, maybe just for the consistency test
                    // dynamicaly generated ids
                    el.remove_attribute("aria-hidden");
                    Ok(())
                }),
                element!(".gem-c-contextual-sidebar", |el| {
                    // this sidebar is not part of the document and can change for unrelated reasons
                    el.remove();
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::default()
        },
    )
    .map_err(|err| format_err!("Error in rewriter : {}", err))
}

fn attachments_from(html: &Html) -> Vec<String> {
    let attachment_selector = Selector::parse(".attachment .title a, .attachment .download a").unwrap();
    let attachments = html
        .select(&attachment_selector)
        .map(|el| el.value().attr("href"))
        .flatten()
        .map(ToString::to_string)
        .collect();
    attachments
}
