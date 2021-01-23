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
- [ ] smtp decoding error also affect the url : https://www.gov..uk/government/publications/phe-analysis-of-transmissibility-based-on-genomics-15-december-2020?utm_medium=email&utm_campaign=govuk-notifications&utm_source=7a5fbaa3-df13-4ea8-a6e0-73f1c8637788&utm_content=daily
- [ ] probably remove that tracking info too
- [ ] add category tags
- [ ] correct the commit message
- [x] "file blocking tree creation"
- [ ] add push
- [ ] clean code
- [ ] replace server

Then db cleanup, including these features
- read the update info from the document
- batch process existing db to find unfetched attachments and add commits

run as daemon:

```sh
date >> logs; bash -c 'setsid cargo run </dev/null &>>logs & jobs -p %1'
```

check daemon:
```sh
lsof logs
lsof -i tcp:25
```

stop daemon:
```sh
killall server
```
