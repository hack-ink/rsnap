#!/usr/bin/env python3

import base64
import binascii
import email.utils
import os
import re
import tempfile
import urllib.parse
import xml.etree.ElementTree as ET
from pathlib import Path


version = os.environ["VERSION"]
tag = os.environ["TAG"]
archive_path = Path(os.environ["ARCHIVE_PATH"])
archive_name = os.environ["ARCHIVE_NAME"]
appcast_path = Path(os.environ["APPCAST"])
signature_output = os.environ["SPARKLE_SIGNATURE_OUTPUT"]

if appcast_path.resolve() == archive_path.resolve():
    raise SystemExit("error: appcast output must not replace the signed release archive")

signature_match = re.fullmatch(
    r'sparkle:edSignature="([A-Za-z0-9+/]+={0,2})" length="([1-9][0-9]*)"',
    signature_output,
)
if signature_match is None:
    raise SystemExit("error: Sparkle sign_update returned an unexpected signature record")

signature, signed_length_text = signature_match.groups()
try:
    signature_bytes = base64.b64decode(signature, validate=True)
except binascii.Error:
    raise SystemExit("error: Sparkle sign_update returned a malformed EdDSA signature")
if len(signature_bytes) != 64:
    raise SystemExit("error: Sparkle sign_update returned an invalid EdDSA signature length")

actual_length = archive_path.stat().st_size
signed_length = int(signed_length_text)
if signed_length != actual_length:
    raise SystemExit(
        f"error: Sparkle signed length {signed_length} does not match archive length {actual_length}"
    )

canonical_download_url = (
    f"https://github.com/acg-box/rsnap/releases/download/{tag}/{archive_name}"
)
canonical_release_url = f"https://github.com/acg-box/rsnap/releases/tag/{tag}"


def checked_url(value: str, canonical: str, label: str) -> str:
    if not value:
        return canonical
    if value == canonical:
        return value

    parsed = urllib.parse.urlsplit(value)
    is_loopback = (
        parsed.scheme in {"http", "https"}
        and parsed.hostname in {"127.0.0.1", "::1", "localhost"}
        and parsed.username is None
        and parsed.password is None
        and not parsed.fragment
    )
    if is_loopback:
        return value
    raise SystemExit(f"error: {label} must use the canonical acg-box/rsnap release URL")


download_url = checked_url(
    os.environ["SPARKLE_ARCHIVE_URL"].strip(),
    canonical_download_url,
    "SPARKLE_ARCHIVE_URL",
)
release_url = checked_url(
    os.environ["SPARKLE_RELEASE_NOTES_URL"].strip(),
    canonical_release_url,
    "SPARKLE_RELEASE_NOTES_URL",
)

sparkle_namespace = "http://www.andymatuschak.org/xml-namespaces/sparkle"
ET.register_namespace("sparkle", sparkle_namespace)

rss = ET.Element("rss", {"version": "2.0"})
channel = ET.SubElement(rss, "channel")
ET.SubElement(channel, "title").text = "Rsnap Updates"
ET.SubElement(channel, "link").text = "https://github.com/acg-box/rsnap/releases"
ET.SubElement(channel, "description").text = "Rsnap macOS app updates."
ET.SubElement(channel, "language").text = "en"

item = ET.SubElement(channel, "item")
ET.SubElement(item, "title").text = f"Version {version}"
ET.SubElement(item, "link").text = release_url
ET.SubElement(item, f"{{{sparkle_namespace}}}version").text = version
ET.SubElement(item, f"{{{sparkle_namespace}}}shortVersionString").text = version
ET.SubElement(item, f"{{{sparkle_namespace}}}minimumSystemVersion").text = "14.0"
ET.SubElement(item, f"{{{sparkle_namespace}}}hardwareRequirements").text = "arm64"
ET.SubElement(item, f"{{{sparkle_namespace}}}releaseNotesLink").text = release_url
ET.SubElement(item, "pubDate").text = email.utils.formatdate(usegmt=True)
ET.SubElement(
    item,
    "enclosure",
    {
        "url": download_url,
        f"{{{sparkle_namespace}}}edSignature": signature,
        "length": str(actual_length),
        "type": "application/octet-stream",
    },
)

tree = ET.ElementTree(rss)
ET.indent(tree, space="  ")
appcast_path.parent.mkdir(parents=True, exist_ok=True)
temporary_path = None
try:
    with tempfile.NamedTemporaryFile(
        mode="wb",
        dir=appcast_path.parent,
        prefix=f".{appcast_path.name}.",
        delete=False,
    ) as temporary_file:
        temporary_path = Path(temporary_file.name)
        tree.write(temporary_file, encoding="utf-8", xml_declaration=True)
    os.replace(temporary_path, appcast_path)
finally:
    if temporary_path is not None and temporary_path.exists():
        temporary_path.unlink()
