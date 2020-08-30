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

## Security

This project is still under active development and is currently being used for research and experimental purposes only, please **DO NOT USE IT IN PRODUCTION** for now.

## License

This project is licensed under MIT license ([LICENSE](./LICENSE) or
http://opensource.org/licenses/MIT)
