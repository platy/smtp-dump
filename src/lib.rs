use std::io::copy;
use ureq::get;
use url::Url;

pub mod email_update;
pub mod doc;
pub use doc::{Doc, DocContent};

pub fn retrieve_doc(url: Url) -> Result<Doc, &'static str> {
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
        let doc = Doc {
            content: DocContent::html(&content, Some(&url))?,
            url: url,
        };

        Ok(doc)
    } else {
        let mut reader = response.into_reader();
        let mut buf = vec![];
        copy(&mut reader, &mut buf).map_err(|err| {
            println!("error : {}, url : {}", &err, &url);
            "Error retrieving attachment"
        })?;
        Ok(Doc {
            url: url,
            content: DocContent::Other(buf),
        })
    }
}
