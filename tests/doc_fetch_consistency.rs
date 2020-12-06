use futures_lite::future;
use gitgov_rs::{remove_ids, retrieve_doc, Doc, DocContent};
use pretty_assertions::assert_eq;

#[test]
fn fetch_and_strip_doc() {
    let (doc, _) = future::block_on(retrieve_doc(
        "https://www.gov.uk/change-name-deed-poll/make-an-adult-deed-poll"
            .parse()
            .unwrap(),
    ))
    .unwrap();
    assert_doc(
        &doc,
        "https://www.gov.uk/change-name-deed-poll/make-an-adult-deed-poll",
        include_str!("govuk/change-name-deed-poll/make-an-adult-deed-poll.html"),
    );
}

#[test]
fn fetch_and_strip_doc_with_attachments() {
    let (doc, attachments) = future::block_on(retrieve_doc(
        "https://www.gov.uk/government/consultations/bus-services-act-2017-bus-open-data"
            .parse()
            .unwrap(),
    ))
    .unwrap();
    assert_doc(
        &doc,
        "https://www.gov.uk/government/consultations/bus-services-act-2017-bus-open-data",
        include_str!("govuk/government/consultations/bus-services-act-2017-bus-open-data.html"),
    );
    assert_eq!(attachments,
        vec![
            "https://assets.publishing.service.gov.uk/government/uploads/system/uploads/attachment_data/file/792313/bus-open-data-consultation-response.pdf".parse().unwrap(), 
            "https://www.gov.uk/government/consultations/bus-services-act-2017-bus-open-data/bus-services-act-2017-bus-open-data-html".parse().unwrap(), 
            "https://assets.publishing.service.gov.uk/government/uploads/system/uploads/attachment_data/file/722573/bus-services-act-2017-open-data-consultation.pdf".parse().unwrap(), 
            "https://assets.publishing.service.gov.uk/government/uploads/system/uploads/attachment_data/file/722576/bus-open-data-case-for-change.pdf".parse().unwrap(),
        ]);
}

#[test]
fn fetch_file() {
    let (doc, attachments) = future::block_on(retrieve_doc(
        "https://assets.publishing.service.gov.uk/government/uploads/system/uploads/attachment_data/file/722576/bus-open-data-case-for-change.pdf".parse().unwrap(),
    ))
    .unwrap();
    assert_file(
        &doc,
        "https://assets.publishing.service.gov.uk/government/uploads/system/uploads/attachment_data/file/722576/bus-open-data-case-for-change.pdf",
        include_bytes!("govuk/government/uploads/system/uploads/attachment_data/file/722576/bus-open-data-case-for-change.pdf"),
    );
    assert!(attachments.is_empty());
}

fn assert_doc(doc: &Doc, url: &str, body: &str) {
    assert_eq!(doc.url.as_str(), url,);
    if let DocContent::DiffableHtml(content) = &doc.content {
        let diff = html_diff::get_differences(content, &remove_ids(body)); // TODO pre strip test data
        assert!(
            diff.is_empty(),
            "Found differences in file at url {} : {:#?}",
            url,
            diff,
        );
    } else {
        panic!("Fail")
    }
}

fn assert_file(doc: &Doc, url: &str, body: &[u8]) {
    assert_eq!(doc.url.as_str(), url,);
    if let DocContent::Other(content) = &doc.content {
        assert_eq!(content.as_slice(), body);
    } else {
        panic!("Fail")
    }
}
