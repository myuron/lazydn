Throwaway keys, generated only for lazydn tests. They protect nothing.
- key.pub, msg1.sig, msg2.sig: made with ssh-keygen outside the repo (ed25519, namespace devnull-api, sha512).
  The matching private key is rebuilt in tests from a 32-byte seed constant (see src/api/test_support.rs).
- rsa_key.b64, enc_key.b64: OpenSSH private key bodies without the BEGIN/END lines; tests add the framing.
  rsa_key is RSA-1024 (rejected: not ed25519); enc_key is ed25519 encrypted with a passphrase (rejected: encrypted).
