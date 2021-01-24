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
- [x] deploy server running alongside existing one
- [x] don't update commit ref until the whole email has been processed
- [x] add category tags
- [x] correct the commit message
- [x] "file blocking tree creation"
- [x] add push
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
