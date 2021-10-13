# TODO

- [x] fetch & strip a regular document
- [x] fetch & strip linked html document
- [x] fetch other document
- [x] read update info from document
- [x] parse email
- [x] listen for emails
- [x] save docs
- [x] make commits
- [x] remove unwraps
- [x] deploy server running alongside existing one
- [x] don't update commit ref until the whole email has been processed
- [x] add category tags
- [x] correct the commit message
- [x] "file blocking tree creation"
- [x] add push
- [x] don't follow links off site
- [ ] deploy properly
- [x] fixed dotstuffing properly
- [x] Timeout the smtp connection after some point if nothing is sent - https://datatracker.ietf.org/doc/html/rfc5321#section-4.5.3.2
- [ ] Write Emails to tempfile and move into place to avoid corrupt files on error
- [ ] Accept multiple incoming SMTP connections at the same time

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

restart:
```sh
cargo build; killall server; date >> logs; bash -c 'setsid cargo run </dev/null &>>logs & jobs -p %1'
```
