# ipfs2filecoin
用于将IPFS文件向Filecoin-miner自动发起交易的中间件

### 使用
#### 依赖
1. `FULLNODE_API_INFO`全局变量
2. `boost`, `boostx`全局变量
3. 确保boost已经完成初始化

#### 命令
```shell
ipfs2filecoin 0.1.0

USAGE:
    ipfs2filecoin.exe [OPTIONS] --miner-id <MINER_ID>

OPTIONS:
    -h, --help                           Print help information
    -i, --ipfs-gateway <IPFS_GATEWAY>    IPFS gateway [default: https://ipfs.io]
    -l, --listen-addr <LISTEN_ADDR>      Server listen addr [default: 0.0.0.0:8888]
    -m, --miner-id <MINER_ID>            Miner id
    -V, --version                        Print version information
```

启动命令样例:
```shell
./ipfs2filecoin -m f01000 -i https://dweb.link
```

#### API
`POST put/:IPFS CID`

```shell
curl -X POST http://127.0.0.1:8888/put/bafybeiflwdj2x5ymjdn5ww2sgzoefcvzdnko4bri3g7kwharnl7xcts4jm
```

预期返回结果:

`HTTP RESPOSE CODE 200`
```json
{
    "deal_uuid": "58a12e34-e0f4-4fe6-ba0e-1d7ce4fd7d85",
    "storage_provider": "f01000",
    "client_wallet": "f3vvlfc4hxqrqmn4a7ha4u3nrjbetqkb3j2mb6xu5ktwuvdjrgju5av4uhnjexh65b73qstfuxippcakgtkqva",
    "payload_cid": "bafybeiflwdj2x5ymjdn5ww2sgzoefcvzdnko4bri3g7kwharnl7xcts4jm",
    "url": "https://dweb.link/api/v0/dag/export?arg=bafybeiflwdj2x5ymjdn5ww2sgzoefcvzdnko4bri3g7kwharnl7xcts4jm",
    "commp": "baga6ea4seaqlx3mi6kccq3x2rw6esuttziwbzpud7bfeu632ppuunjofhmf7ody",
    "start_epoch": 26418,
    "end_epoch": 544818,
    "provider_collateral": "0"
}
```

### 构建
```shell
cargo build --release
```
