#!/usr/bin/env python3
"""Fetches what H3 lets an agent read beside the game into walkthroughs/, which git ignores.

    python3 references.py [sgl] [gbatek] [walkthroughs]      (no argument: all three)

- sgl/: the SGL User's Manual of Sega's Saturn documentation (1997), its Japanese pages, which are the
  originals; one request at a time with a pause.
- gbatek/: Martin Korth's GBATEK, the page and a plain-text copy for searching.
- telago-golden-sun.txt, telago-golden-sun-the-lost-age.txt: the two GameFAQs walkthroughs. GameFAQs answers
  programs with a browser check, and this script does not work around it: where the plain request is refused it
  says which page to save by hand, and where.

Nothing here is committed, quoted beyond a phrase, or copied elsewhere: each stays its author's.
"""
import html.parser
import pathlib
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

OUT = pathlib.Path(__file__).resolve().parent / "walkthroughs"
SGL = "https://docs.exodusemulator.com/Archives/SSDDV25/segahtml/"
GBATEK = "https://problemkaputt.de/gbatek.htm"
WALKTHROUGHS = [
    ("telago-golden-sun.txt", "https://gamefaqs.gamespot.com/gba/468548-golden-sun/faqs/39216"),
    ("telago-golden-sun-the-lost-age.txt", "https://gamefaqs.gamespot.com/gba/561356-golden-sun-the-lost-age/faqs/44287"),
]
SGL_PAGE_LIMIT = 400


def fetch(url):
    """The page's bytes, or None with the reason printed."""
    request = urllib.request.Request(url, headers={"User-Agent": "references.py"})
    try:
        with urllib.request.urlopen(request, timeout=40) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        print(f"  {error.code} {url}")
    except OSError as error:
        print(f"  {error} {url}")
    return None


class Text(html.parser.HTMLParser):
    """A page's text, without its scripts and styles."""

    def __init__(self):
        super().__init__()
        self.parts, self.skip = [], 0

    def handle_starttag(self, tag, attrs):
        if tag in ("script", "style"):
            self.skip += 1
        elif tag in ("br", "p", "tr", "li", "h1", "h2", "h3", "pre", "table"):
            self.parts.append("\n")

    def handle_endtag(self, tag):
        if tag in ("script", "style") and self.skip:
            self.skip -= 1

    def handle_data(self, data):
        if not self.skip:
            self.parts.append(data)


def text_of(page):
    parser = Text()
    parser.feed(page)
    return "".join(parser.parts)


def sgl():
    """The Japanese pages reachable from sgl/index_j.htm, inside sgl/ only, breadth first."""
    root = OUT / "sgl"
    queue, seen, kept = ["sgl/index_j.htm"], {"sgl/index_j.htm"}, 0
    while queue and kept < SGL_PAGE_LIMIT:
        path = queue.pop(0)
        body = fetch(SGL + path)
        time.sleep(0.5)
        if body is None:
            continue
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(body)
        kept += 1
        page = body.decode("shift_jis", errors="replace")
        for link in re.findall(r'(?i)(?:href|src)\s*=\s*["\']?([^"\'\s>#]+)', page):
            joined = urllib.parse.urljoin(SGL + path, link)
            if not joined.startswith(SGL + "sgl/") or not joined.lower().endswith("_j.htm"):
                continue
            relative = joined[len(SGL):]
            if relative not in seen:
                seen.add(relative)
                queue.append(relative)
    print(f"sgl: {kept} Japanese pages under {root}")
    return kept > 0


def gbatek():
    body = fetch(GBATEK)
    if body is None:
        return False
    folder = OUT / "gbatek"
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "gbatek.htm").write_bytes(body)
    (folder / "gbatek.txt").write_text(text_of(body.decode("latin-1")), encoding="utf-8")
    print(f"gbatek: {len(body):,} bytes, and gbatek.txt, under {folder}")
    return True


def walkthroughs():
    done = True
    for name, url in WALKTHROUGHS:
        target = OUT / name
        if target.is_file():
            print(f"{name}: already there")
            continue
        body = fetch(url)
        page = body.decode("utf-8", errors="replace") if body else ""
        guide = re.search(r'(?s)<pre[^>]*id="faqtext"[^>]*>(.*?)</pre>', page)
        if guide:
            target.write_text(text_of(guide.group(1)), encoding="utf-8")
            print(f"{name}: written")
        else:
            done = False
            print(f"{name}: the site asks for a browser. Open {url} in one, save the guide's text, and put it at {target}")
    return done


def main():
    steps = {"sgl": sgl, "gbatek": gbatek, "walkthroughs": walkthroughs}
    wanted = sys.argv[1:] or list(steps)
    unknown = [name for name in wanted if name not in steps]
    if unknown:
        sys.exit(f"unknown: {' '.join(unknown)}; choose from {' '.join(steps)}")
    OUT.mkdir(exist_ok=True)
    results = [steps[name]() for name in wanted]
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
