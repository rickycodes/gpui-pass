# Yubico attestation trust snapshot

Downloaded 2026-11-12 from Yubico's published PKI:

- https://developers.yubico.com/PKI/yubico-ca-certs.txt
- https://developers.yubico.com/PKI/yubico-ca-certs.txt.sig
- https://developers.yubico.com/PKI/yubico-intermediate.pem

The source trust bundle's OpenPGP signature was verified against primary key
`57A9DEED4C6D962A923BB691816F3ED99921835E` (Emil Lundberg), listed at
https://developers.yubico.com/Software_Projects/Software_Signing.html.
The signing subkey was `EAA09128E15582099347E5D3C81FDCBBE03F92DF`.

Source bundle SHA-256:
`3c2b23c699399c89fe3a93e3b2688149d60e042132138010f246c3600804c604`

`yubico-fido-roots.pem` contains the following three certificates extracted from
that verified bundle:

- Yubico U2F Root CA Serial 457200631
- Yubico FIDO Root CA Serial 450203556
- Yubico Attestation Root 1

`yubico-intermediates.pem` is the downloaded intermediate bundle, used only as
untrusted chain-building material. An intermediate is accepted only when OpenSSL
validates its path to one of the three pinned roots. System trust roots and
certificate downloads are disabled during device verification.

These are public certificates published by Yubico for relying-party attestation
verification. The snapshot is compiled into the application. Updates should
repeat source signature verification against Yubico's published signing keys;
preserve the existing roots needed for older supported keys and update this
provenance record.
