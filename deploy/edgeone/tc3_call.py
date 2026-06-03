#!/usr/bin/env python3
"""Minimal TC3-HMAC-SHA256 signer for Tencent Cloud Teo (EdgeOne) API.

Reads creds from env: TENCENTCLOUD_SECRET_ID / TENCENTCLOUD_SECRET_KEY.
No secrets are stored here. Usage:

    python3 tc3_call.py <Action> '<json-payload>' [region]

Defaults: service=teo, version=2022-09-01, host=teo.tencentcloudapi.com
"""
import sys
import os
import json
import time
import hashlib
import hmac
import urllib.request
import urllib.error

SERVICE = os.environ.get("TC3_SERVICE", "teo")
HOST = os.environ.get("TC3_HOST", "teo.tencentcloudapi.com")
VERSION = os.environ.get("TC3_VERSION", "2022-09-01")
REGION = os.environ.get("TC3_REGION", "")


def sign(key, msg):
    return hmac.new(key, msg.encode("utf-8"), hashlib.sha256).digest()


def main():
    action = sys.argv[1]
    payload = sys.argv[2] if len(sys.argv) > 2 else "{}"
    region = sys.argv[3] if len(sys.argv) > 3 else REGION

    secret_id = os.environ["TENCENTCLOUD_SECRET_ID"]
    secret_key = os.environ["TENCENTCLOUD_SECRET_KEY"]

    # validate payload is json
    json.loads(payload)

    timestamp = int(time.time())
    date = time.strftime("%Y-%m-%d", time.gmtime(timestamp))

    # canonical request
    http_method = "POST"
    canonical_uri = "/"
    canonical_qs = ""
    ct = "application/json; charset=utf-8"
    canonical_headers = (
        f"content-type:{ct}\n"
        f"host:{HOST}\n"
        f"x-tc-action:{action.lower()}\n"
    )
    signed_headers = "content-type;host;x-tc-action"
    hashed_payload = hashlib.sha256(payload.encode("utf-8")).hexdigest()
    canonical_request = (
        f"{http_method}\n{canonical_uri}\n{canonical_qs}\n"
        f"{canonical_headers}\n{signed_headers}\n{hashed_payload}"
    )

    # string to sign
    algorithm = "TC3-HMAC-SHA256"
    credential_scope = f"{date}/{SERVICE}/tc3_request"
    hashed_canonical = hashlib.sha256(
        canonical_request.encode("utf-8")
    ).hexdigest()
    string_to_sign = (
        f"{algorithm}\n{timestamp}\n{credential_scope}\n{hashed_canonical}"
    )

    # signature
    secret_date = sign(("TC3" + secret_key).encode("utf-8"), date)
    secret_service = sign(secret_date, SERVICE)
    secret_signing = sign(secret_service, "tc3_request")
    signature = hmac.new(
        secret_signing, string_to_sign.encode("utf-8"), hashlib.sha256
    ).hexdigest()

    authorization = (
        f"{algorithm} Credential={secret_id}/{credential_scope}, "
        f"SignedHeaders={signed_headers}, Signature={signature}"
    )

    headers = {
        "Authorization": authorization,
        "Content-Type": ct,
        "Host": HOST,
        "X-TC-Action": action,
        "X-TC-Timestamp": str(timestamp),
        "X-TC-Version": VERSION,
    }
    if region:
        headers["X-TC-Region"] = region

    req = urllib.request.Request(
        f"https://{HOST}",
        data=payload.encode("utf-8"),
        headers=headers,
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            body = resp.read().decode("utf-8")
            print(body)
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8")
        print(f"HTTP {e.code}\n{body}", file=sys.stderr)
        print(body)
        sys.exit(1)


if __name__ == "__main__":
    main()
