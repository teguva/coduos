//! In-tunnel DNS on 10.8.0.1:53 (Firefly uses 8.8.8.8; we need .home).
//!
//! iptables DNAT of packets to a local address is unreliable, and nothing else
//! listens on port 53. This stub answers CoduOS proxy names with 10.8.0.1 and
//! forwards everything else to the LAN resolver.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::timeout;

use crate::config::Config;
use crate::vpn::{self, CLIENT_DNS};

const QTYPE_A: u16 = 1;
const QTYPE_AAAA: u16 = 28;

pub fn spawn(cfg: Config) {
    tokio::spawn(async move {
        run(cfg).await;
    });
}

async fn run(cfg: Config) {
    let bind = SocketAddr::V4(SocketAddrV4::new(
        CLIENT_DNS.parse().unwrap_or(Ipv4Addr::new(10, 8, 0, 1)),
        53,
    ));
    loop {
        if !vpn::enabled(&cfg) {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }
        match UdpSocket::bind(bind).await {
            Ok(sock) => {
                tracing::info!("vpn dns listening on {bind}");
                let sock = Arc::new(sock);
                if let Err(err) = serve(sock, &cfg).await {
                    tracing::warn!("vpn dns: {err}");
                }
            }
            Err(err) => {
                tracing::debug!("vpn dns bind {bind}: {err}");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

async fn serve(sock: Arc<UdpSocket>, cfg: &Config) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                if !vpn::enabled(cfg) {
                    return Ok(());
                }
            }
            result = sock.recv_from(&mut buf) => {
                let (n, peer) = result?;
                let pkt = buf[..n].to_vec();
                let sock = Arc::clone(&sock);
                let cfg = cfg.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle(&sock, peer, &pkt, &cfg).await {
                        tracing::debug!("vpn dns query from {peer}: {err}");
                    }
                });
            }
        }
    }
}

async fn handle(
    sock: &UdpSocket,
    peer: SocketAddr,
    pkt: &[u8],
    cfg: &Config,
) -> Result<(), String> {
    let Some((name, qtype)) = question_name_type(pkt) else {
        return Ok(());
    };
    if let Some(reply) = local_reply(pkt, &name, qtype, cfg) {
        sock.send_to(&reply, peer)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let Some(up) = vpn::dns_upstream() else {
        return Err("no DNS upstream".into());
    };
    let reply = forward(&up, pkt).await?;
    sock.send_to(&reply, peer)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn local_reply(pkt: &[u8], name: &str, qtype: u16, cfg: &Config) -> Option<Vec<u8>> {
    let ip: Ipv4Addr = CLIENT_DNS.parse().ok()?;
    let known = crate::proxy::enabled_hostnames(cfg);
    if !known.iter().any(|h| h == name) {
        return None;
    }
    match qtype {
        QTYPE_A => build_a(pkt, ip),
        QTYPE_AAAA => build_nodata(pkt),
        _ => None,
    }
}

async fn forward(upstream: &str, pkt: &[u8]) -> Result<Vec<u8>, String> {
    let sock = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| e.to_string())?;
    sock.connect((upstream, 53))
        .await
        .map_err(|e| e.to_string())?;
    sock.send(pkt).await.map_err(|e| e.to_string())?;
    let mut buf = [0u8; 4096];
    let n = timeout(Duration::from_secs(2), sock.recv(&mut buf))
        .await
        .map_err(|_| "upstream DNS timeout".to_string())?
        .map_err(|e| e.to_string())?;
    Ok(buf[..n].to_vec())
}

fn question_name_type(msg: &[u8]) -> Option<(String, u16)> {
    if msg.len() < 12 {
        return None;
    }
    let qd = u16::from_be_bytes([msg[4], msg[5]]);
    if qd == 0 {
        return None;
    }
    let (name, off) = parse_name(msg, 12).ok()?;
    if off + 4 > msg.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([msg[off], msg[off + 1]]);
    Some((name, qtype))
}

fn parse_name(msg: &[u8], mut off: usize) -> Result<(String, usize), ()> {
    let mut labels = Vec::new();
    let mut jumped = false;
    let mut end = off;
    let mut hops = 0;
    loop {
        if off >= msg.len() || hops > 10 {
            return Err(());
        }
        let len = msg[off];
        if len & 0xC0 == 0xC0 {
            if off + 1 >= msg.len() {
                return Err(());
            }
            let ptr = (((len as usize) & 0x3F) << 8) | msg[off + 1] as usize;
            if !jumped {
                end = off + 2;
            }
            off = ptr;
            jumped = true;
            hops += 1;
            continue;
        }
        if len & 0xC0 != 0 {
            return Err(());
        }
        if len == 0 {
            if !jumped {
                end = off + 1;
            }
            break;
        }
        off += 1;
        let next = off + len as usize;
        if next > msg.len() {
            return Err(());
        }
        labels.push(String::from_utf8_lossy(&msg[off..next]).to_ascii_lowercase());
        off = next;
    }
    Ok((labels.join("."), end))
}

fn question_end(query: &[u8]) -> Option<usize> {
    let (_, name_end) = parse_name(query, 12).ok()?;
    let end = name_end + 4;
    if end > query.len() {
        None
    } else {
        Some(end)
    }
}

fn build_a(query: &[u8], ip: Ipv4Addr) -> Option<Vec<u8>> {
    header_and_question(query, true).map(|mut out| {
        out.extend_from_slice(&[0xC0, 0x0C]);
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&30u32.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&ip.octets());
        out
    })
}

fn build_nodata(query: &[u8]) -> Option<Vec<u8>> {
    header_and_question(query, false)
}

fn header_and_question(query: &[u8], with_answer: bool) -> Option<Vec<u8>> {
    if query.len() < 12 {
        return None;
    }
    let qend = question_end(query)?;
    let rd = query[2] & 0x01;
    let mut out = Vec::with_capacity(qend + 16);
    out.extend_from_slice(&query[0..2]);
    out.push(0x84 | rd);
    out.push(0x80);
    out.extend_from_slice(&query[4..6]);
    out.extend_from_slice(&(if with_answer { 1u16 } else { 0u16 }).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&query[12..qend]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query_for(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q
    }

    #[test]
    fn parses_immich_home() {
        let q = query_for("immich.home", QTYPE_A);
        let (name, qtype) = question_name_type(&q).unwrap();
        assert_eq!(name, "immich.home");
        assert_eq!(qtype, QTYPE_A);
    }

    #[test]
    fn answers_a_record() {
        let q = query_for("immich.home", QTYPE_A);
        let ip = Ipv4Addr::new(10, 8, 0, 1);
        let r = build_a(&q, ip).unwrap();
        assert_eq!(&r[0..2], &[0x12, 0x34]);
        assert_eq!(r[2] & 0x80, 0x80);
        assert_eq!(&r[r.len() - 4..], &ip.octets());
        let (name, _) = question_name_type(&r).unwrap();
        assert_eq!(name, "immich.home");
    }
}
