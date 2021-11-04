# SMTP-dump

Listens for SMTP connections, accepts them, and writes any emails that come through them into an inbox on the filesystem.

There are 3 uses that I know of for this:

* You are writing a small application that needs to receive some emails and it is inconvienient to run an SMTP server and much easier to just read emails from the filesystem
* You are doing spam research
* You want to see if your machine has a public IP without a firewall, because within 24 hours someone will send spam via you

## Install:

```sh
cargo install smtp-dump
```

## Run as daemon:

```sh
date >> logs; bash -c 'setsid smtp-dump </dev/null &>>logs & jobs -p %1'
```

### Check daemon:

```sh
lsof logs
lsof -i tcp:25
```

### Stop daemon:

```sh
killall smtp-dump
```

### Restart:

```sh
killall smtp-dump; date >> logs; bash -c 'setsid cargo run </dev/null &>>logs & jobs -p %1'
```
