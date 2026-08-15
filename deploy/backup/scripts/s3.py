#!/usr/bin/env python3
"""Generic S3-compatible transfer using SigV4.

Configuration is endpoint/region/bucket/prefix/access-key/secret-key plus an
optional session token. There are no provider-specific APIs or object formats.
"""

from __future__ import annotations

import datetime as dt
import hashlib
import hmac
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from typing import Iterable


class ConfigError(SystemExit):
    pass


def env(name: str, default: str | None = None, required: bool = False) -> str:
    value = os.environ.get(name, default)
    if required and not value:
        raise ConfigError(f"missing {name}")
    return value or ""


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def hmac_sha256(key: bytes, msg: str) -> bytes:
    return hmac.new(key, msg.encode("utf-8"), hashlib.sha256).digest()


def signing_key(secret: str, datestamp: str, region: str, service: str) -> bytes:
    k_date = hmac_sha256(("AWS4" + secret).encode("utf-8"), datestamp)
    k_region = hmac.new(k_date, region.encode("utf-8"), hashlib.sha256).digest()
    k_service = hmac.new(k_region, service.encode("utf-8"), hashlib.sha256).digest()
    return hmac.new(k_service, b"aws4_request", hashlib.sha256).digest()


class S3Client:
    def __init__(self) -> None:
        self.endpoint = env("FOYER_BACKUP_S3_ENDPOINT", required=True).rstrip("/")
        self.region = env("FOYER_BACKUP_S3_REGION", "us-east-1") or "us-east-1"
        self.bucket = env("FOYER_BACKUP_S3_BUCKET", required=True)
        prefix = env("FOYER_BACKUP_S3_PREFIX", "")
        prefix = prefix.lstrip("/")
        if prefix and not prefix.endswith("/"):
            prefix += "/"
        self.prefix = prefix
        self.access_key = env("FOYER_BACKUP_S3_ACCESS_KEY", required=True)
        self.secret_key = env("FOYER_BACKUP_S3_SECRET_KEY", required=True)
        self.session_token = env("FOYER_BACKUP_S3_SESSION_TOKEN", "")
        addressing = env("FOYER_BACKUP_S3_ADDRESSING", "path") or "path"
        if addressing not in {"path", "virtual"}:
            raise ConfigError("FOYER_BACKUP_S3_ADDRESSING must be path or virtual")
        self.addressing = addressing
        parsed = urllib.parse.urlparse(self.endpoint)
        if parsed.scheme not in {"http", "https"} or not parsed.netloc:
            raise ConfigError("FOYER_BACKUP_S3_ENDPOINT must be an http(s) URL")
        self.scheme = parsed.scheme
        self.host = parsed.netloc

    def _canonical_uri(self, key: str) -> str:
        quoted = urllib.parse.quote(key, safe="/-_.~")
        if self.addressing == "virtual":
            return "/" + quoted if quoted else "/"
        return f"/{self.bucket}/{quoted}" if quoted else f"/{self.bucket}"

    def _host_header(self) -> str:
        if self.addressing == "virtual":
            return f"{self.bucket}.{self.host}"
        return self.host

    def _url(self, key: str, query: str = "") -> str:
        host = self._host_header()
        uri = self._canonical_uri(key)
        if query:
            return f"{self.scheme}://{host}{uri}?{query}"
        return f"{self.scheme}://{host}{uri}"

    def request(
        self,
        method: str,
        key: str,
        query: str = "",
        body: bytes = b"",
        content_type: str = "application/octet-stream",
        extra_headers: dict[str, str] | None = None,
    ) -> tuple[int, bytes, dict[str, str]]:
        now = dt.datetime.now(dt.timezone.utc)
        amz_date = now.strftime("%Y%m%dT%H%M%SZ")
        datestamp = now.strftime("%Y%m%d")
        payload_hash = sha256_hex(body)
        headers = {
            "host": self._host_header(),
            "x-amz-content-sha256": payload_hash,
            "x-amz-date": amz_date,
        }
        if self.session_token:
            headers["x-amz-security-token"] = self.session_token
        if body or method in {"PUT", "POST"}:
            headers["content-type"] = content_type
            headers["content-length"] = str(len(body))
        if extra_headers:
            headers.update(extra_headers)
        signed_header_names = ";".join(sorted(headers))
        canonical_headers = "".join(
            f"{name}:{headers[name]}\n" for name in sorted(headers)
        )
        canonical_request = "\n".join(
            [
                method,
                self._canonical_uri(key),
                query,
                canonical_headers,
                signed_header_names,
                payload_hash,
            ]
        )
        credential_scope = f"{datestamp}/{self.region}/s3/aws4_request"
        string_to_sign = "\n".join(
            [
                "AWS4-HMAC-SHA256",
                amz_date,
                credential_scope,
                sha256_hex(canonical_request.encode("utf-8")),
            ]
        )
        signature = hmac.new(
            signing_key(self.secret_key, datestamp, self.region, "s3"),
            string_to_sign.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()
        headers["authorization"] = (
            "AWS4-HMAC-SHA256 "
            f"Credential={self.access_key}/{credential_scope}, "
            f"SignedHeaders={signed_header_names}, "
            f"Signature={signature}"
        )
        request = urllib.request.Request(
            self._url(key, query),
            data=body if method in {"PUT", "POST"} else None,
            method=method,
            headers=headers,
        )
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                payload = response.read()
                return response.status, payload, dict(response.headers)
        except urllib.error.HTTPError as error:
            payload = error.read()
            return error.code, payload, dict(error.headers)

    def ensure_bucket(self) -> None:
        status, body, _ = self.request("PUT", "", body=b"", content_type="")
        if status in {200, 204}:
            print(f"bucket ready: {self.bucket}", file=sys.stderr)
            return
        if status in {409, 405}:
            print(f"bucket already present: {self.bucket}", file=sys.stderr)
            return
        raise SystemExit(f"ensure-bucket failed ({status}): {body.decode('utf-8', 'replace')}")

    def head(self, key: str) -> int:
        status, body, headers = self.request("HEAD", key)
        if status != 200:
            raise SystemExit(f"head failed ({status}): {body.decode('utf-8', 'replace')}")
        try:
            length = headers.get("Content-Length") or headers.get("content-length")
            if length is None:
                return -1
            return int(length)
        except ValueError:
            return -1

    def put(self, local_path: str, key: str) -> None:
        with open(local_path, "rb") as handle:
            body = handle.read()
        status, response, _ = self.request("PUT", key, body=body)
        if status not in {200, 204}:
            raise SystemExit(f"put failed ({status}): {response.decode('utf-8', 'replace')}")
        remote_size = self.head(key)
        if remote_size >= 0 and remote_size != len(body):
            raise SystemExit(
                f"upload verification failed: local {len(body)} bytes, remote {remote_size} bytes"
            )
        print(key)

    def get(self, key: str, local_path: str) -> None:
        status, body, _ = self.request("GET", key)
        if status != 200:
            raise SystemExit(f"get failed ({status}): {body.decode('utf-8', 'replace')}")
        parent = os.path.dirname(local_path)
        if parent:
            os.makedirs(parent, mode=0o700, exist_ok=True)
        with open(local_path, "wb") as handle:
            handle.write(body)
        os.chmod(local_path, 0o600)
        print(local_path)

    def list(self, prefix: str | None = None) -> Iterable[str]:
        token = None
        wanted = self.prefix if prefix is None else prefix
        while True:
            params = {"list-type": "2"}
            if wanted:
                params["prefix"] = wanted
            if token:
                params["continuation-token"] = token
            query = "&".join(
                f"{urllib.parse.quote(key, safe='-_.~')}={urllib.parse.quote(value, safe='-_.~')}"
                for key, value in sorted(params.items())
            )
            status, body, _ = self.request("GET", "", query=query)
            if status != 200:
                raise SystemExit(
                    f"list failed ({status}): {body.decode('utf-8', 'replace')}"
                )
            root = ET.fromstring(body)
            ns = ""
            if root.tag.startswith("{"):
                ns = root.tag.split("}")[0] + "}"
            for contents in root.findall(f"{ns}Contents"):
                key = contents.findtext(f"{ns}Key")
                size = contents.findtext(f"{ns}Size") or "0"
                modified = contents.findtext(f"{ns}LastModified") or ""
                if key:
                    print(f"{key}\t{size}\t{modified}")
                    yield key
            is_truncated = (root.findtext(f"{ns}IsTruncated") or "").lower() == "true"
            token = root.findtext(f"{ns}NextContinuationToken")
            if not is_truncated or not token:
                break


def usage() -> None:
    print(
        "usage: s3.py put LOCAL KEY | get KEY LOCAL | head KEY | list [PREFIX] | ensure-bucket",
        file=sys.stderr,
    )
    raise SystemExit(2)


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        usage()
    client = S3Client()
    command = argv[1]
    if command == "put" and len(argv) == 4:
        client.put(argv[2], argv[3])
        return 0
    if command == "get" and len(argv) == 4:
        client.get(argv[2], argv[3])
        return 0
    if command == "list":
        list(client.list(argv[2] if len(argv) == 3 else None))
        return 0
    if command == "head" and len(argv) == 3:
        print(client.head(argv[2]))
        return 0
    if command == "ensure-bucket" and len(argv) == 2:
        client.ensure_bucket()
        return 0
    usage()
    return 2


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv))
    except ConfigError as error:
        print(f"error: {error}", file=sys.stderr)
        raise
