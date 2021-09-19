use anyhow::Result;
use std::{
    collections::VecDeque,
    env::args_os,
    fs::{create_dir_all, File},
    io::Write,
    path::Path,
};
use url::Url;

use gitgov_rs::retrieve_doc;

fn main() -> Result<()> {
    let args: Vec<_> = args_os().collect();
    let url: Url = args
        .get(1)
        .expect("Url to fetch should be first argument")
        .to_str()
        .unwrap()
        .parse()?;
    let dir = args
        .get(2)
        .expect("base directory to store files should be second argument")
        .to_str()
        .unwrap();

    let mut urls: VecDeque<Url> = VecDeque::new();
    urls.push_back(url);

    while let Some(url) = urls.pop_front() {
        let doc = retrieve_doc(&url)?;
        urls.extend(doc.content.attachments().unwrap_or_default().iter().cloned());

        let mut path = Path::new(&dir).join(doc.url.path().strip_prefix('/').unwrap());
        if doc.content.is_html() {
            assert!(path.set_extension("html"));
        }
        let _ = create_dir_all(path.parent().unwrap());
        println!("Writing doc to : {}", path.to_str().unwrap());
        let mut file = File::create(path)?;
        file.write_all(doc.content.as_bytes())?;
    }
    Ok(())
}
