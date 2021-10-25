
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
