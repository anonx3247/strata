"""Text helpers the Strata program in this folder calls (`extern python`)."""

import re
import unicodedata


def slugify(title):
    """A title as a URL slug: lowercase ASCII words joined by dashes."""
    ascii_title = unicodedata.normalize("NFKD", title).encode("ascii", "ignore").decode()
    return re.sub(r"[^a-z0-9]+", "-", ascii_title.lower()).strip("-")


def word_counts(text):
    """How many times each word appears, by word."""
    counts = {}
    for word in re.findall(r"[a-z']+", text.lower()):
        counts[word] = counts.get(word, 0) + 1
    return counts


def summary(post):
    """A post (a dict with "title", "body", "tags") summed up as a Card."""
    words = post["body"].split()
    return {
        "type": "Card",
        "slug": slugify(post["title"]),
        "words": len(words),
        "teaser": " ".join(words[:5]),
        "tags": sorted(set(post["tags"])),
    }


def parse_year(text):
    """The year in `text`: raises ValueError when there is none."""
    match = re.search(r"\b(1[0-9]{3}|20[0-9]{2})\b", text)
    if not match:
        raise ValueError(f"no year in {text!r}")
    return int(match.group(1))
