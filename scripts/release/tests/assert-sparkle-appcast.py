#!/usr/bin/env python3

import sys
import xml.etree.ElementTree as ET


path, expected_signature, expected_archive_url, expected_notes_url = sys.argv[1:]
namespace = {"sparkle": "http://www.andymatuschak.org/xml-namespaces/sparkle"}
root = ET.parse(path).getroot()
assert root.tag == "rss"
channel = root.find("channel")
assert channel is not None
assert channel.findtext("link") == "https://github.com/acg-box/rsnap/releases"
item = channel.find("item")
assert item is not None
assert item.findtext("sparkle:version", namespaces=namespace) == "1.2.3"
assert (
    item.findtext("sparkle:releaseNotesLink", namespaces=namespace)
    == expected_notes_url
)
enclosure = item.find("enclosure")
assert enclosure is not None
assert enclosure.attrib["url"] == expected_archive_url
assert enclosure.attrib["length"] == "9"
assert enclosure.attrib[
    "{http://www.andymatuschak.org/xml-namespaces/sparkle}edSignature"
] == expected_signature
