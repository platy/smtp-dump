# TODO

- [x] fetch & strip a regular document
- [x] fetch & strip linked html document
- [x] fetch other document
- [x] read update info from document
- [x] parse email
- [x] listen for emails
- [x] save docs
- [x] make commits
- [ ] server integration test
- [x] remove unwraps
- [ ] deploy server running alongside existing one
- [x] don't update commit ref until the whole email has been processed
- [ ] fix the smtp encoding leftovers in the commit message
- [ ] correct the commit message
- [ ] "file blocking tree creation"
- [ ] add push
- [ ] clean code
- [ ] replace server

Then db cleanup, including these features
- read the update info from the document
- batch process existing db to find unfetched attachments and add commits
