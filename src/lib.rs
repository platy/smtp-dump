use lol_html::{element, rewrite_str, RewriteStrSettings};
use scraper::{Html, Selector};
use surf::{get, Url};

#[derive(Debug, Eq, PartialEq)]
pub struct Doc {
    pub url: Url,
    pub content: DocContent,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DocContent {
    DiffableHtml(String),
    Other(Vec<u8>),
}

impl DocContent {
    fn html(html: &Html) -> Result<Self, &'static str> {
        let main_selector = Selector::parse("main").unwrap();
        let main = html.select(&main_selector).next().ok_or("No main found")?;
        Ok(DocContent::DiffableHtml(remove_ids(&main.html())))
    }

    pub fn is_html(&self) -> bool {
        match self {
            Self::DiffableHtml(_) => true,
            Self::Other(_) => false,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            DocContent::DiffableHtml(string) => string.as_bytes(),
            DocContent::Other(bytes) => bytes.as_slice(),
        }
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

pub async fn retrieve_doc(url: Url) -> Result<(Doc, Vec<Url>), &'static str> {
    // TODO return the doc and the urls of attachments, probably remove async, I can just use a thread pool and worker queue
    println!("retrieving url : {}", &url);
    let mut response = get(&url).send().await.map_err(|_| "Error retrieving")?;

    if response
        .content_type()
        .map_or(false, |mime| mime.essence() == "text/html")
    {
        let content = response.body_string().await.map_err(|err| {
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
        Ok((
            Doc {
                url: url.to_owned(),
                content: DocContent::Other(response.body_bytes().await.map_err(|err| {
                    println!("error : {}, url : {}", &err, &url);
                    "Error retrieving attachment"
                })?),
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
