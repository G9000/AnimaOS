# M63 synthetic TLS fixtures

These files exist only for finite loopback tests of the pinned HTTPS browser
transport. Never install this CA or use the fixture server key in an application.

- `ca.pem`: synthetic test CA certificate; its signing key was discarded.
- `server-key.pem`: synthetic server key shared by these test certificates.
- `server.pem`: CA-signed certificate for `m63-browser.anima.tools`.
- `wrong-name.pem`: same CA and key, with `wrong-browser.anima.tools` as its name.
- `untrusted.pem`: self-signed certificate for `m63-browser.anima.tools`.

Generated locally with existing OpenSSL 3.2.4, SHA-256 signatures and RSA 2048-bit
keys. Long validity avoids fixture expiry. Tests inject trust and loopback dialing
only through their dependency seam; production configuration accepts neither.
The fixture hostnames and public-looking IP pin are never contacted.
