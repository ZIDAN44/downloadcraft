//! Asking the router to forward a port, so peers out on the internet can connect in: the packets
//! of NAT-PMP (RFC 6886) and the requests of UPnP's Internet Gateway Device (SSDP discovery, its
//! description, and `AddPortMapping` / `DeletePortMapping` over SOAP), each written from its
//! specification. Nothing here does I/O; `net` sends them.

/// The port a NAT-PMP router listens on.
pub const NATPMP_PORT: u16 = 5351;

/// A transport a port is forwarded for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Udp,
}

impl Proto {
    pub fn name(self) -> &'static str {
        match self {
            Proto::Tcp => "TCP",
            Proto::Udp => "UDP",
        }
    }
    fn natpmp_op(self) -> u8 {
        match self {
            Proto::Udp => 1,
            Proto::Tcp => 2,
        }
    }
}

/// A NAT-PMP mapping request: forward `port` outside to `port` here for `lifetime` seconds (0
/// takes the mapping away).
pub fn natpmp_request(proto: Proto, port: u16, lifetime: u32) -> [u8; 12] {
    let mut p = [0u8; 12];
    p[1] = proto.natpmp_op();
    p[4..6].copy_from_slice(&port.to_be_bytes());
    p[6..8].copy_from_slice(&port.to_be_bytes());
    p[8..12].copy_from_slice(&lifetime.to_be_bytes());
    p
}

/// A NAT-PMP answer to [`natpmp_request`]: the outside port and how long the router keeps it.
pub fn natpmp_answer(b: &[u8], proto: Proto) -> Result<(u16, u32), String> {
    if b.len() < 16 {
        return Err("the router's answer is too short".into());
    }
    if b.first() != Some(&0) || b.get(1) != Some(&(128 + proto.natpmp_op())) {
        return Err("the router answered something else".into());
    }
    let word = |i: usize| b.get(i..i + 2).map_or(0, |x| u16::from_be_bytes([x[0], x[1]]));
    let long = |i: usize| b.get(i..i + 4).map_or(0, |x| u32::from_be_bytes([x[0], x[1], x[2], x[3]]));
    match word(2) {
        0 => Ok((word(10), long(12))),
        1 => Err("the router doesn't take this version of NAT-PMP".into()),
        2 => Err("the router has port forwarding turned off".into()),
        3 => Err("the router can't reach the internet".into()),
        4 => Err("the router has no ports left".into()),
        n => Err(format!("the router refused (NAT-PMP code {n})")),
    }
}

/// The SSDP search for a UPnP router (a multicast to 239.255.255.250:1900).
pub fn ssdp_search() -> &'static [u8] {
    b"M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n"
}

/// Where an SSDP answer says the router describes itself (its `LOCATION`), when it's an
/// `http://` address.
pub fn ssdp_location(answer: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(answer);
    let line = text.lines().find(|l| l.len() > 9 && l.get(..9).is_some_and(|k| k.eq_ignore_ascii_case("location:")))?;
    let url = line.get(9..)?.trim();
    (url.starts_with("http://") && url.len() < 512).then(|| url.to_string())
}

/// The WAN connection service a router's description offers: its type and its control address
/// (as written; relative to the description's address when it doesn't start with `http`).
pub fn igd_service(description: &str) -> Option<(String, String)> {
    for kind in ["WANIPConnection:2", "WANIPConnection:1", "WANPPPConnection:1"] {
        let service = format!("urn:schemas-upnp-org:service:{kind}");
        let mut rest = description;
        while let Some(at) = rest.find("<serviceType>") {
            rest = rest.get(at + 13..)?;
            let end = rest.find("</serviceType>")?;
            let here = rest.get(..end)?.trim();
            let block_end = rest.find("</service>").unwrap_or(rest.len());
            let block = rest.get(..block_end)?;
            if here == service
                && let Some(c) = block.find("<controlURL>")
            {
                let after = block.get(c + 12..)?;
                let url = after.get(..after.find("</controlURL>")?)?.trim();
                return Some((service, url.to_string()));
            }
        }
    }
    None
}

/// The full address of `url` as given in a description at `base` (`http://host:port/…`).
pub fn join_url(base: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    let after = base.strip_prefix("http://").unwrap_or(base);
    let host = after.split('/').next().unwrap_or("");
    format!("http://{host}{}{url}", if url.starts_with('/') { "" } else { "/" })
}

/// A UPnP `AddPortMapping` request for `service`: `port` outside to `client:port`, for `lease`
/// seconds (0: until taken away). The SOAP body, and the `SOAPAction` header to send with it.
pub fn upnp_add(service: &str, proto: Proto, port: u16, client: &str, lease: u32) -> (String, String) {
    let body = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:AddPortMapping xmlns:u=\"{service}\"><NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort><NewProtocol>{}</NewProtocol><NewInternalPort>{port}</NewInternalPort><NewInternalClient>{client}</NewInternalClient><NewEnabled>1</NewEnabled><NewPortMappingDescription>DownloadCraft {port} {}</NewPortMappingDescription><NewLeaseDuration>{lease}</NewLeaseDuration></u:AddPortMapping></s:Body></s:Envelope>",
        proto.name(),
        proto.name().to_ascii_lowercase()
    );
    (body, format!("\"{service}#AddPortMapping\""))
}

/// A UPnP `DeletePortMapping` request, as [`upnp_add`].
pub fn upnp_delete(service: &str, proto: Proto, port: u16) -> (String, String) {
    let body = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:DeletePortMapping xmlns:u=\"{service}\"><NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort><NewProtocol>{}</NewProtocol></u:DeletePortMapping></s:Body></s:Envelope>",
        proto.name()
    );
    (body, format!("\"{service}#DeletePortMapping\""))
}

/// The UPnP error code in a SOAP fault (`725`: the router takes only mappings without a lease).
pub fn upnp_error(body: &str) -> Option<u32> {
    let at = body.find("<errorCode>")?;
    let rest = body.get(at + 11..)?;
    rest.get(..rest.find("</errorCode>")?)?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natpmp_packets_follow_rfc_6886() {
        let r = natpmp_request(Proto::Tcp, 47895, 7200);
        assert_eq!(r, [0, 2, 0, 0, 0xBB, 0x17, 0xBB, 0x17, 0, 0, 0x1C, 0x20]);
        assert_eq!(natpmp_request(Proto::Udp, 1, 0)[1], 1);
        let ok = [0, 130, 0, 0, 0, 0, 0, 9, 0xBB, 0x17, 0xBB, 0x17, 0, 0, 0x1C, 0x20];
        assert_eq!(natpmp_answer(&ok, Proto::Tcp), Ok((47895, 7200)));
        assert!(natpmp_answer(&ok, Proto::Udp).is_err(), "an answer for the other transport");
        let mut off = ok;
        off[3] = 2;
        assert_eq!(natpmp_answer(&off, Proto::Tcp), Err("the router has port forwarding turned off".into()));
        assert!(natpmp_answer(&ok[..12], Proto::Tcp).is_err());
    }

    #[test]
    fn upnp_routers_are_found_and_asked() {
        let answer = b"HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\nLocation: http://10.1.0.1:1900/twjwm/rootDesc.xml\r\nST: urn:x\r\n\r\n";
        assert_eq!(ssdp_location(answer).as_deref(), Some("http://10.1.0.1:1900/twjwm/rootDesc.xml"));
        assert_eq!(ssdp_location(b"HTTP/1.1 200 OK\r\nLOCATION: file:///etc/x\r\n"), None);
        let desc = "<root><service><serviceType>urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1</serviceType><controlURL>/ctl/CmnIfCfg</controlURL></service>\
            <service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType><SCPDURL>/x.xml</SCPDURL><controlURL>/ctl/IPConn</controlURL></service></root>";
        let (service, ctl) = igd_service(desc).unwrap();
        assert_eq!((service.as_str(), ctl.as_str()), ("urn:schemas-upnp-org:service:WANIPConnection:1", "/ctl/IPConn"));
        assert_eq!(join_url("http://10.1.0.1:1900/twjwm/rootDesc.xml", &ctl), "http://10.1.0.1:1900/ctl/IPConn");
        assert_eq!(join_url("http://10.1.0.1:1900/a", "http://10.1.0.1:5000/c"), "http://10.1.0.1:5000/c");
        assert_eq!(igd_service("<root></root>"), None);
        let (body, action) = upnp_add(&service, Proto::Udp, 47895, "10.1.3.3", 3600);
        assert!(body.contains("<NewProtocol>UDP</NewProtocol>") && body.contains("<NewInternalClient>10.1.3.3</NewInternalClient>"));
        assert_eq!(action, "\"urn:schemas-upnp-org:service:WANIPConnection:1#AddPortMapping\"");
        assert!(upnp_delete(&service, Proto::Tcp, 47895).0.contains("<NewExternalPort>47895</NewExternalPort>"));
        assert_eq!(upnp_error("<s:Fault><detail><UPnPError><errorCode>725</errorCode></UPnPError></detail></s:Fault>"), Some(725));
    }
}
