# Clover Protocol

An aSVC based stateless protocol as L2 rollup on [Nervos CKB](https://www.nervos.org/).

[![License: MIT](https://flat.badgen.net/badge/license/MIT/orange)](./LICENSE)

## About Clover Protocol

[Whitepaper](./clover-protocol.md)

## Play With it

### build

1. compile contract

```
capsule build --release
```

### Have Fun

1. Run mock of ckb dev chain

```sh
cargo run --bin mock-ckb
```

2. Run layer2 node

```sh
cargo run --bin mock-ckb
```

3. Install test-tools

```sh
pip install httpie
```

4. Deploy contract

```sh
http POST 127.0.0.1:8001/setup
```

5. Register accounts

```sh
http POST 127.0.0.1:8001/register pubkey=00 psk=00
http POST 127.0.0.1:8001/register pubkey=01 psk=01
```

6. Deposit

```sh
http POST 127.0.0.1:8001/deposit to=0 amount=1000 psk=00
```

7. Transfer

```sh
http POST 127.0.0.1:8001/transfer from=0 to=1 amount=10 psk=00
```

8. Withdraw

```sh
 http POST 127.0.0.1:8001/withdraw from=0 amount=99 psk=00
```

## Security

This project is still under active development and is currently being used for research and experimental purposes only, please **DO NOT USE IT IN PRODUCTION** for now.

## License

This project is licensed under MIT license ([LICENSE](./LICENSE) or
http://opensource.org/licenses/MIT)
