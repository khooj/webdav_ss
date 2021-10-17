#!/usr/bin/env bash

# generate root ca based on private key
openssl req -x509 -nodes -newkey rsa:2048 -keyout ./private-key.key -out server.crt -days 365
# generate certificate signing request for subject alternative names (for localhost domain support)
openssl req -nodes -newkey rsa:2048 -keyout san-private-key.key -out san_server.csr
# generate certificate with SAN
openssl x509 -req -in san_server.csr -CA server.crt -CAkey private-key.key -out san_server.crt -CAcreateserial -days 365 -sha256 -extfile server_cert_ext.cnf

# convert keys to rsa format
openssl rsa -in private-key.key -out privatersa-key.key
openssl rsa -in san-private-key.key -out san-privatersa-key.key
