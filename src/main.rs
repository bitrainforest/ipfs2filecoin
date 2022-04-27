use std::fmt::{Debug, Formatter};
use std::io;
use std::io::{BufRead, Cursor};
use std::mem::MaybeUninit;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use anyhow::anyhow;
use clap::Parser;
use serde::Serialize;
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use warp::reply::Json;
use warp::{Filter, Rejection};

#[derive(Parser)]
#[clap(version)]
struct Args {
    /// Server listen addr
    #[clap(short, long, default_value_t = SocketAddr::from((IpAddr::from([0, 0, 0, 0]), 8888)))]
    listen_addr: SocketAddr,
    /// IPFS gateway
    #[clap(short, long, default_value_t = String::from("https://ipfs.io"))]
    ipfs_gateway: String,
    /// Miner id
    #[clap(short, long)]
    miner_id: String,
}

static mut ARGS: MaybeUninit<Args> = MaybeUninit::uninit();

fn set_args(args: Args) {
    unsafe {
        ARGS.write(args);
    }
}

fn get_args() -> &'static Args {
    unsafe { ARGS.assume_init_ref() }
}

struct DealCMD {
    provider: String,
    http_url: String,
    commp: String,
    car_size: usize,
    piece_size: usize,
    payload_cid: String,
    storage_price_per_epoch: usize,
    verified: bool,
}

struct CommpRes {
    commp_cid: String,
    piece_size: usize,
    car_file_size: usize,
}

#[derive(Serialize)]
struct DealRes {
    deal_uuid: String,
    storage_provider: String,
    client_wallet: String,
    payload_cid: String,
    url: String,
    commp: String,
    start_epoch: i64,
    end_epoch: i64,
    provider_collateral: String,
}

struct CustomReject(anyhow::Error);

impl Debug for CustomReject {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl warp::reject::Reject for CustomReject {}

fn custom_reject(error: impl Into<anyhow::Error>) -> Rejection {
    warp::reject::custom(CustomReject(error.into()))
}

async fn read_ipfs_to_local(ipfs_url: &str) -> anyhow::Result<NamedTempFile> {
    let mut resp = reqwest::get(ipfs_url).await?;

    let (temp_file, file_fd) = tokio::task::spawn_blocking(move || {
        let temp_file = NamedTempFile::new()?;
        let file_fd = temp_file.reopen()?;
        Result::<(NamedTempFile, std::fs::File), io::Error>::Ok((temp_file, file_fd))
    })
    .await??;

    let mut local_file = File::from_std(file_fd);

    while let Some(chunk) = resp.chunk().await? {
        local_file.write_all(&chunk).await?
    }
    Ok(temp_file)
}

macro_rules! read_line {
    ($lines: expr, $err_msg: expr) => {{
        let mut f = || -> anyhow::Result<String> {
            let line = $lines
                .next()
                .ok_or_else(|| anyhow!($err_msg))?
                .map_err(|_| anyhow!($err_msg))?;
            Ok(line)
        };
        f()
    }};
}

macro_rules! resolve_next_line {
    ($match: expr, $lines: expr, $err_msg: expr) => {{
        let mut f = || -> anyhow::Result<String> {
            let line = read_line!($lines, $err_msg)?;
            let line = line.trim();
            let v = sscanf::scanf!(line, $match, str).map_err(|_| anyhow!($err_msg))?;
            Ok(v.trim().to_string())
        };
        f()
    }};
}

async fn commp(file: NamedTempFile) -> anyhow::Result<CommpRes> {
    let output = tokio::process::Command::new("boostx")
        .arg("commp")
        .arg(file.path())
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow!(String::from_utf8(output.stderr)?));
    }

    let mut lines = Cursor::new(output.stdout).lines();
    let commp_cid: String =
        resolve_next_line!("CommP CID: {}", lines, "Resolve commp cid failure")?;

    const RESOLVE_PIECE_SIZE_FAILURE: &str = "Resolve piece size failure";
    let piece_size: String =
        resolve_next_line!("Piece size: {}", lines, RESOLVE_PIECE_SIZE_FAILURE)?;
    let piece_size =
        usize::from_str(&piece_size).map_err(|_| anyhow!(RESOLVE_PIECE_SIZE_FAILURE))?;

    const RESOLVE_CAR_FILE_SIZE_FAILURE: &str = "Resolve car file size failure";
    let car_file_size: String =
        resolve_next_line!("Car file size: {}", lines, RESOLVE_CAR_FILE_SIZE_FAILURE)?;
    let car_file_size =
        usize::from_str(&car_file_size).map_err(|_| anyhow!(RESOLVE_CAR_FILE_SIZE_FAILURE))?;

    let commp_res = CommpRes {
        commp_cid,
        piece_size,
        car_file_size,
    };
    Ok(commp_res)
}

async fn deal(mut cmd: DealCMD) -> anyhow::Result<DealRes> {
    let output = loop {
        let output = tokio::process::Command::new("boost")
            .arg("deal")
            .args(["--provider", &cmd.provider])
            .args(["--http-url", &cmd.http_url])
            .args(["--commp", &cmd.commp])
            .args(["--car-size", &cmd.car_size.to_string()])
            .args(["--piece-size", &cmd.piece_size.to_string()])
            .args(["--payload-cid", &cmd.payload_cid])
            .args([
                "--storage-price-per-epoch",
                &cmd.storage_price_per_epoch.to_string(),
            ])
            .arg(format!("--verified={}", cmd.verified))
            .output()
            .await?;

        if !output.status.success() {
            let err = String::from_utf8(output.stderr)?;

            if err.contains("storage price per epoch less than asking price") {
                let str = err.split(':').last().ok_or_else(|| anyhow!(err.clone()))?;
                let str = str.trim();
                let storage_price_per_epoch: String =
                    sscanf::scanf!(str, "0 < {}", String).map_err(|_| anyhow!(err.clone()))?;
                cmd.storage_price_per_epoch =
                    usize::from_str(storage_price_per_epoch.trim()).map_err(|_| anyhow!(err))?;
                continue;
            } else {
                return Err(anyhow!(err));
            }
        }
        break output;
    };

    let mut lines = Cursor::new(output.stdout).lines();
    const RESOLVE_STDOUT_FAILURE: &str = "Resolve stdout failure";

    // expected sent deal proposal
    let _ = read_line!(lines, RESOLVE_STDOUT_FAILURE)?;

    const RESOLVE_DEAL_UUID_FAILURE: &str = "Resolve deal uuid failure";
    let deal_uuid = resolve_next_line!("deal uuid: {}", lines, RESOLVE_DEAL_UUID_FAILURE)?;

    // storage provider
    let _ = read_line!(lines, RESOLVE_STDOUT_FAILURE)?;

    let client_wallet =
        resolve_next_line!("client wallet: {}", lines, "Resolve client wallet failure")?;

    // payload cid
    let _ = read_line!(lines, RESOLVE_STDOUT_FAILURE)?;

    // http url
    let _ = read_line!(lines, RESOLVE_STDOUT_FAILURE)?;

    let commp = resolve_next_line!("commp: {}", lines, "Resolve CommP failure")?;

    const RESOLVE_START_EPOCH_FAILURE: &str = "Resolve start epoch failure";
    let start_epoch = resolve_next_line!("start epoch: {}", lines, RESOLVE_START_EPOCH_FAILURE)?;
    let start_epoch =
        i64::from_str(&start_epoch).map_err(|_| anyhow!(RESOLVE_START_EPOCH_FAILURE))?;

    const RESOLVE_END_EPOCH_FAILURE: &str = "Resolve end epoch failure";
    let end_epoch = resolve_next_line!("end epoch: {}", lines, RESOLVE_END_EPOCH_FAILURE)?;
    let end_epoch = i64::from_str(&end_epoch).map_err(|_| anyhow!(RESOLVE_END_EPOCH_FAILURE))?;

    let provider_collateral = resolve_next_line!(
        "provider collateral: {}",
        lines,
        "Resolve provider collateral failure"
    )?;

    let res = DealRes {
        deal_uuid,
        storage_provider: cmd.provider,
        client_wallet,
        payload_cid: cmd.payload_cid,
        url: cmd.http_url,
        commp,
        start_epoch,
        end_epoch,
        provider_collateral,
    };
    Ok(res)
}

async fn handler(cid: String) -> Result<Json, Rejection> {
    let fut = async move {
        let ipfs_url = format!("{}/api/v0/dag/export?arg={}", get_args().ipfs_gateway, cid);
        let file = read_ipfs_to_local(&ipfs_url).await?;
        let commp = commp(file).await?;

        let cmd = DealCMD {
            provider: get_args().miner_id.clone(),
            http_url: ipfs_url,
            commp: commp.commp_cid,
            car_size: commp.car_file_size,
            piece_size: commp.piece_size,
            payload_cid: cid,
            storage_price_per_epoch: 0,
            verified: false,
        };

        let res = deal(cmd).await?;
        Result::<Json, anyhow::Error>::Ok(warp::reply::json(&res))
    };

    fut.await.map_err(custom_reject)
}

#[tokio::main]
async fn main() {
    set_args(Args::parse());
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let promote = warp::post()
        .and(warp::path("put"))
        .and(warp::path::param())
        .and_then(handler);

    warp::serve(promote).run(get_args().listen_addr).await
}
