use std::io::copy;

use chrono::{DateTime, Utc};
use lol_html::{element, rewrite_str, RewriteStrSettings};
use scraper::{Html, Selector};
use ureq::get;
use url::Url;

#[derive(Debug, Eq, PartialEq)]
pub struct Doc {
    pub url: Url,
    pub content: DocContent,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DocContent {
    DiffableHtml(String, Vec<DocUpdate>),
    Other(Vec<u8>),
}

#[derive(Debug, Eq, PartialEq)]
pub struct DocUpdate(DateTime<Utc>, String);

impl DocContent {
    fn html(html: &Html) -> Result<Self, &'static str> {
        let main_selector: Selector = Selector::parse("main").unwrap();
        let main = html.select(&main_selector).next().ok_or("No main found")?;
        let history_selector: Selector = Selector::parse("#full-history li").unwrap();
        let time_selector: Selector = Selector::parse("time").unwrap();
        let p_selector: Selector = Selector::parse("p").unwrap();
        let history = html
            .select(&history_selector)
            .map(|history_elem| {
                DocUpdate(
                    history_elem
                        .select(&time_selector)
                        .next()
                        .unwrap()
                        .value()
                        .attr("datetime")
                        .unwrap()
                        .parse()
                        .unwrap(),
                    history_elem
                        .select(&p_selector)
                        .next()
                        .unwrap()
                        .inner_html(),
                )
            })
            .collect();
        Ok(DocContent::DiffableHtml(remove_ids(&main.html()), history))
    }

    pub fn is_html(&self) -> bool {
        match self {
            Self::DiffableHtml(_, _) => true,
            Self::Other(_) => false,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            DocContent::DiffableHtml(string, _) => string.as_bytes(),
            DocContent::Other(bytes) => bytes.as_slice(),
        }
    }

    pub fn history(&self) -> Option<&[DocUpdate]> {
        match self {
            DocContent::DiffableHtml(_, history) => Some(history.as_slice()),
            DocContent::Other(_) => None,
        }
    }
}

impl DocUpdate {
    pub fn new(date: DateTime<Utc>, summary: impl Into<String>) -> Self {
        Self(date, summary.into())
    }
}

pub fn remove_ids(html: &str) -> String {
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
    .unwrap()
}

pub fn retrieve_doc(url: Url) -> Result<(Doc, Vec<Url>), &'static str> {
    // TODO return the doc and the urls of attachments, probably remove async, I can just use a thread pool and worker queue
    println!("retrieving url : {}", &url);
    let response = get(&url.as_str()).call();
    if let Some(_err) = response.synthetic_error() {
        return Err("Error retrieving");
    }

    if response.content_type() == "text/html" {
        let content = response.into_string().map_err(|err| {
            println!("error : {}, url : {}", &err, &url);
            "Error retrieveing document"
        })?;
        let html = Html::parse_document(&content);
        let doc = Doc {
            url: url.clone(),
            content: DocContent::html(&html)?,
        };

        let attachments = attachments(&html)
            .into_iter()
            .map(|a_url| url.join(&a_url).unwrap())
            .collect();
        Ok((doc, attachments))
    } else {
        let mut reader = response.into_reader();
        let mut buf = vec![];
        copy(&mut reader, &mut buf).map_err(|err| {
            println!("error : {}, url : {}", &err, &url);
            "Error retrieving attachment"
        })?;
        Ok((
            Doc {
                url: url,
                content: DocContent::Other(buf),
            },
            vec![],
        ))
    }
}

fn attachments(html: &Html) -> Vec<String> {
    let attachment_selector =
        Selector::parse(".attachment .title a, .attachment .download a").unwrap();
    let attachments = html
        .select(&attachment_selector)
        .map(|el| el.value().attr("href"))
        .flatten()
        .map(ToString::to_string)
        .collect();
    attachments
}
