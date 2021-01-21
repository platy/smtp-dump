use anyhow::{bail, format_err, Context, Result};
use std::io::copy;
use ureq::get;
use url::Url;

pub mod doc;
pub mod email_update;
pub use doc::{Doc, DocContent};
pub mod git;

pub fn retrieve_doc(url: Url) -> Result<Doc> {
    // TODO return the doc and the urls of attachments, probably remove async, I can just use a thread pool and worker queue
    println!("retrieving url : {}", &url);
    let response = get(&url.as_str()).call();
    if let Some(_err) = response.synthetic_error() {
        bail!("Error retrieving");
    }

    if response.content_type() == "text/html" {
        let content = response.into_string().with_context(|| url.clone())?;
        let doc = Doc {
            content: DocContent::html(&content, Some(&url))?,
            url,
        };

        Ok(doc)
    } else {
        let mut reader = response.into_reader();
        let mut buf = vec![];
        copy(&mut reader, &mut buf)
            .map_err(|err| format_err!("Error retrieving attachment : {}, url : {}", &err, &url))?;
        Ok(Doc {
            url,
            content: DocContent::Other(buf),
        })
    }
}
