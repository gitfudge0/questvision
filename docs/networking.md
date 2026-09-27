# Network and TLS

Quest Display is designed for a direct LAN connection. The host serves its own web page and signaling; WebRTC carries media on locally negotiated ICE candidates. No cloud relay, public STUN server, or TURN server is planned for normal same-LAN use. The host IP must be reachable from the Quest, and the firewall must allow the listening TCP port plus the UDP ports that WebRTC selects.

## Address and exposure

Prefer a private LAN address printed by the host. Guest Wi-Fi, client isolation, separate VLANs, VPN routing, or a captive portal may prevent ICE from connecting even when the HTTPS page opens. Do not forward the HTTPS or WebRTC ports on a router. Do not expose a development HTTP mode to other devices.

The code selects a detected private LAN IP and TCP port 47990 by default, with loopback as a fallback if no private address is found. Override the interface and port with `questdisplay --listen <private-ip> --port <port> start`. The application does not expose a fixed UDP port range. mDNS is not implemented; use the IP URL.

## Local certificates

The first run creates a local self-signed certificate. The host prints its SHA-256 fingerprint. Browsers do not automatically trust it. Compare the browser certificate fingerprint with the host value if the browser exposes it, or install a trusted local CA/certificate through the browser's supported method. Quest Browser's certificate trust UX has not been tested. An address check alone does not authenticate the host, and a warning exception may still fail to provide every secure-context capability. [MDN explains WebXR's secure context requirement](https://developer.mozilla.org/en-US/docs/Web/API/WebXR_Device_API/Startup_and_shutdown).

Pairing authenticates the application session after TLS connects; it does not make an unverified TLS identity safe. No public certificate authority can ordinarily validate an arbitrary private IP merely because it is on a LAN.
