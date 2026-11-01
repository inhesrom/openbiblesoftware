#!/usr/bin/env python3
"""PROTOTYPE data prep for the app shell prototype (issue #13). Throwaway.

Downloads open sources into ./.cache and writes small JSON extracts into ./data:

  kjv.json    Genesis, eBible eng-kjv2006 (PD), Strong's from USFM \\w attributes
  tahot.json  Genesis, STEPBible TAHOT (CC BY 4.0, STEP Bible, www.STEPBible.org)
  jfb.json    Genesis, Jamieson-Fausset-Brown via CrossWire JFB SWORD module (PD)
  mhcc.json   Genesis, Matthew Henry Concise via CrossWire MHCC SWORD module (PD)
  xref.json   Genesis cross-refs, OpenBible.info (CC BY 4.0) + KJV text of targets
  fonts       Gentium Plus 6.200 + Ezra SIL 2.51 (OFL) into ./assets/fonts

Shape loosely follows ADR 0003: text units of plain NFC text + standoff spans.
Changes made to STEPBible data: only Genesis extracted; morpheme separators removed;
grammar codes expanded to English.
"""
import html, io, json, os, re, struct, sys, unicodedata, urllib.request, zipfile, zlib
from collections import Counter, defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, ".cache")
DATA = os.path.join(HERE, "data")
FONTS = os.path.join(HERE, "assets", "fonts")
os.makedirs(CACHE, exist_ok=True)
os.makedirs(DATA, exist_ok=True)
os.makedirs(FONTS, exist_ok=True)

SRC = {
    "kjv2006.zip": "https://eBible.org/Scriptures/eng-kjv2006_usfm.zip",
    "xref.zip": "https://a.openbible.info/data/cross-references.zip",
    "tahot.txt": "https://raw.githubusercontent.com/STEPBible/STEPBible-Data/master/Translators%20Amalgamated%20OT%2BNT/TAHOT%20Gen-Deu%20-%20Translators%20Amalgamated%20Hebrew%20OT%20-%20STEPBible.org%20CC%20BY.txt",
    "JFB.zip": "https://crosswire.org/ftpmirror/pub/sword/packages/rawzip/JFB.zip",
    "MHCC.zip": "https://crosswire.org/ftpmirror/pub/sword/packages/rawzip/MHCC.zip",
    "gentium.zip": "https://software.sil.org/downloads/r/gentium/GentiumPlus-6.200.zip",
    "ezra.zip": "https://software.sil.org/downloads/r/ezra/EzraSIL-2.51.zip",
}


def fetch(name):
    p = os.path.join(CACHE, name)
    if not os.path.exists(p):
        print("download", SRC[name])
        req = urllib.request.Request(SRC[name], headers={"User-Agent": "obs-prototype"})
        with urllib.request.urlopen(req) as r, open(p, "wb") as f:
            f.write(r.read())
    return p


nfc = lambda s: unicodedata.normalize("NFC", s)

# OSIS ids of the 66-book canon, in order (KJV / Standard versification).
BOOKS = "Gen Exod Lev Num Deut Josh Judg Ruth 1Sam 2Sam 1Kgs 2Kgs 1Chr 2Chr Ezra Neh Esth Job Ps Prov Eccl Song Isa Jer Lam Ezek Dan Hos Joel Amos Obad Jonah Mic Nah Hab Zeph Hag Zech Mal Matt Mark Luke John Acts Rom 1Cor 2Cor Gal Eph Phil Col 1Thess 2Thess 1Tim 2Tim Titus Phlm Heb Jas 1Pet 2Pet 1John 2John 3John Jude Rev".split()
USFM_IDS = "GEN EXO LEV NUM DEU JOS JDG RUT 1SA 2SA 1KI 2KI 1CH 2CH EZR NEH EST JOB PSA PRO ECC SNG ISA JER LAM EZK DAN HOS JOL AMO OBA JON MIC NAM HAB ZEP HAG ZEC MAL MAT MRK LUK JHN ACT ROM 1CO 2CO GAL EPH PHP COL 1TH 2TH 1TI 2TI TIT PHM HEB JAS 1PE 2PE 1JN 2JN 3JN JUD REV".split()
USFM2OSIS = dict(zip(USFM_IDS, BOOKS))


class Unit:
    def __init__(self, kind):
        self.kind = kind
        self.text = ""
        self.spans = []  # [start, end, kind, data]

    def add(self, s):
        start = len(self.text.encode())
        self.text += s
        return start, len(self.text.encode())

    def pos(self):
        return len(self.text.encode())

    def json(self):
        return {"kind": self.kind, "text": self.text, "spans": self.spans}


# ---------------------------------------------------------------- KJV + Strong's
def parse_usfm_book(raw, book, tagged=True):
    """Returns (units, verse_texts) for one USFM book."""
    units, plain = [], {}
    cur = None
    chapter = 0
    verse_span = None
    open_spans = []  # stack of [kind, start, data]

    def new_unit(kind):
        nonlocal cur
        close_verse()
        cur = Unit(kind)
        units.append(cur)

    def close_verse():
        nonlocal verse_span
        if verse_span is not None and cur is not None:
            verse_span[1] = cur.pos()
            if verse_span[1] > verse_span[0]:
                cur.spans.append(verse_span)
            verse_span = None

    tok = re.compile(r"(\\\+?[a-z0-9]+\*?)")
    raw = raw.replace("\r", "")
    for line in raw.split("\n"):
        line = line.rstrip()
        m = re.match(r"\\(id|h|toc\d|mt\d?)\b", line)
        if m:
            continue
        m = re.match(r"\\c (\d+)", line)
        if m:
            close_verse()
            chapter = int(m.group(1))
            u = Unit("chapter")
            s, e = u.add(f"{BOOK_NAMES[book]} {chapter}" if True else "")
            u.spans.append([s, e, "chapter", {"c": chapter}])
            units.append(u)
            cur = None
            continue
        if line.startswith("\\p") or line.startswith("\\q") or line.startswith("\\m"):
            kind = "q" if line.startswith("\\q") else "p"
            vs = verse_span
            new_unit(kind)
            if vs is not None:  # paragraph break inside a verse: continue verse span
                verse_span = [cur.pos(), None, "verse", vs[3]]
            line = re.sub(r"^\\[pqm]\d?\s*", "", line)
            if not line:
                continue
        if cur is None:
            new_unit("p")
        # tokenise
        rest = line
        m = re.match(r"\\v (\d+)\s*", rest)
        if m:
            close_verse()
            v = int(m.group(1))
            ref = f"{book}.{chapter}.{v}"
            if cur.text and not cur.text.endswith(" "):
                cur.add(" ")
            verse_span = [cur.pos(), None, "verse", {"ref": ref, "label": str(v)}]
            plain.setdefault(ref, "")
            rest = rest[m.end():]
        cur_ref = verse_span[3]["ref"] if verse_span else None
        # footnotes -> zero-width note spans
        parts = re.split(r"(\\f .*?\\f\*)", rest)
        for part in parts:
            if part.startswith("\\f "):
                ft = re.sub(r"\\fr [^\\]*", "", part)
                ft = re.sub(r"\\f\*|\\f \+|\\ft ", "", ft).strip()
                p = cur.pos()
                cur.spans.append([p, p, "note", {"text": nfc(ft)}])
                continue
            pieces = tok.split(part)
            skip_space = False
            for pc in pieces:
                if not pc:
                    continue
                if pc.startswith("\\"):
                    name = pc.lstrip("\\+").rstrip("*")
                    closing = pc.endswith("*")
                    if not closing:
                        open_spans.append([name, cur.pos(), None])
                        skip_space = True
                    else:
                        for i in range(len(open_spans) - 1, -1, -1):
                            if open_spans[i][0] == name:
                                k, s, d = open_spans.pop(i)
                                e = cur.pos()
                                if name == "w":
                                    if tagged and d:
                                        cur.spans.append([s, e, "w", d])
                                elif name == "add":
                                    cur.spans.append([s, e, "add", None])
                                elif name == "nd":
                                    cur.spans.append([s, e, "nd", None])
                                break
                    continue
                txt = pc
                if skip_space and txt.startswith(" "):
                    txt = txt[1:]
                skip_space = False
                if open_spans and open_spans[-1][0] == "w" and "|" in txt:
                    word, attrs = txt.split("|", 1)
                    sm = re.search(r'strong="([^"]+)"', attrs)
                    if sm:
                        open_spans[-1][2] = {"s": sm.group(1)}
                    txt = word
                txt = nfc(txt)
                cur.add(txt)
                if cur_ref:
                    plain[cur_ref] = plain.get(cur_ref, "") + txt
        if cur.text and not cur.text.endswith(" "):
            cur.add(" ")
            if cur_ref:
                plain[cur_ref] += " "
    close_verse()
    for u in units:
        u.text = u.text.rstrip()
        n = len(u.text.encode())
        for sp in u.spans:
            sp[0] = min(sp[0], n)
            sp[1] = min(sp[1], n)
    plain = {k: re.sub(r"\s+", " ", v).replace("¶ ", "").strip() for k, v in plain.items()}
    return units, plain


BOOK_NAMES = {"Gen": "Genesis"}


def kjv():
    z = zipfile.ZipFile(fetch("kjv2006.zip"))
    all_plain = {}
    gen_units = None
    for n in z.namelist():
        m = re.match(r"\d+-([0-9A-Z]{3})eng-kjv2006\.usfm$", os.path.basename(n))
        if not m or m.group(1) not in USFM2OSIS:
            continue
        book = USFM2OSIS[m.group(1)]
        BOOK_NAMES.setdefault(book, book)
        raw = z.read(n).decode("utf-8-sig")
        units, plain = parse_usfm_book(raw, book, tagged=(book == "Gen"))
        all_plain.update(plain)
        if book == "Gen":
            gen_units = units
    out = {
        "id": "kjv-strongs",
        "abbrev": "KJV",
        "title": "King James Version with Strong's (eng-kjv2006)",
        "type": "bible",
        "lang": "en",
        "dir": "ltr",
        "attribution": "Public Domain. eBible.org eng-kjv2006, 'with Strong's numbers added'.",
        "units": [u.json() for u in gen_units],
    }
    json.dump(out, open(os.path.join(DATA, "kjv.json"), "w"), ensure_ascii=False)
    return all_plain


# ---------------------------------------------------------------- TAHOT
STEMS = dict(q="Qal", N="Niphal", p="Piel", P="Pual", h="Hiphil", H="Hophal", t="Hithpael", o="Polel", O="Polal",
             r="Hithpolel", m="Poel", M="Poal", k="Palel", K="Pulal", Q="Qal passive", l="Pilpel", L="Polpal",
             f="Hithpalpel", D="Nithpael", j="Pealal", i="Pilel", u="Hothpaal", c="Tiphil", v="Hishtaphel",
             w="Nithpalel", y="Nithpoel", z="Hithpoel")
ASTEMS = dict(q="Peal", Q="Peil", u="Hithpeel", p="Pael", P="Ithpaal", M="Hithpaal", a="Aphel", h="Haphel",
              s="Saphel", e="Shaphel", H="Hophal", i="Ithpeel", t="Hishtaphel", v="Ishtaphel", w="Hithaphel",
              o="Polel", z="Ithpoel", r="Hithpolel", f="Hithpalpel", b="Hephal", c="Tiphel", m="Poel",
              l="Palpel", L="Ithpalpel", O="Ithpolel", G="Ittaphal")
VTYPE = dict(p="perfect", q="sequential perfect", i="imperfect", w="sequential imperfect", h="cohortative",
             j="jussive", v="imperative", r="active participle", s="passive participle", a="infinitive absolute",
             c="infinitive construct")
PERSON = {"1": "1st person", "2": "2nd person", "3": "3rd person"}
GENDER = dict(m="masculine", f="feminine", b="both genders", c="common")
NUMBER = dict(s="singular", p="plural", d="dual")
STATE = dict(a="absolute", c="construct", d="determined")


def pgn(s):
    out = []
    for ch in s:
        for tbl in (PERSON, GENDER, NUMBER):
            if ch in tbl:
                out.append(tbl[ch])
                break
    return out


def morph_seg(seg, aramaic):
    if not seg:
        return ""
    p, r = seg[0], seg[1:]
    if p == "V":
        stem = (ASTEMS if aramaic else STEMS).get(r[:1], r[:1])
        t = VTYPE.get(r[1:2], r[1:2])
        rest = r[2:]
        bits = pgn(rest[:3]) if t not in ("active participle", "passive participle") else [GENDER.get(rest[:1], ""), NUMBER.get(rest[1:2], ""), STATE.get(rest[2:3], "")]
        return " ".join(x for x in ["Verb", stem, t] + bits if x)
    if p == "N":
        kind = dict(c="common", g="gentilic", p="proper", x="unknown").get(r[:1], "")
        if r[:1] == "p":
            sub = dict(m="person", f="person (fem.)", l="location", t="title").get(r[1:2], "")
            return f"Proper noun {sub}".strip()
        return " ".join(x for x in ["Noun", kind, GENDER.get(r[1:2], ""), NUMBER.get(r[2:3], ""), STATE.get(r[3:4], "")] if x)
    if p == "A":
        kind = dict(a="", c="cardinal number", g="gentilic", o="ordinal number").get(r[:1], "")
        return " ".join(x for x in ["Adjective", kind, GENDER.get(r[1:2], ""), NUMBER.get(r[2:3], ""), STATE.get(r[3:4], "")] if x)
    if p == "P":
        kind = dict(d="demonstrative", f="indefinite", i="interrogative", p="personal", r="relative").get(r[:1], "")
        return " ".join(["Pronoun", kind] + pgn(r[1:]))
    if p == "S":
        kind = dict(d="directional he", h="paragogic he", n="paragogic nun", p="pronominal").get(r[:1], "")
        return " ".join(["Suffix", kind] + pgn(r[1:]))
    if p == "T":
        kind = dict(a="affirmation", d="definite article", e="exhortation", i="interrogative", j="interjection",
                    m="demonstrative", n="negative", o="direct object marker", r="relative").get(r[:1], "")
        return ("Particle " + kind).strip() if kind != "definite article" else "Definite article"
    if p == "R":
        return "Preposition" + (" + definite article" if r[:1] == "d" else "")
    if p == "C":
        return "Conjunction"
    if p == "c":
        return "Conjunction (vav consecutive)"
    if p == "D":
        return "Adverb"
    return seg


def morph_en(code):
    aramaic = code.startswith("A")
    code = code[1:] if code[:1] in "HA" else code
    return " + ".join(morph_seg(s, aramaic) for s in code.split("/") if s)


def tahot():
    words, units, lex = [], [], {}
    usage = defaultdict(Counter)
    cur = None
    cur_chapter = 0
    cur_verse_std = None
    verse_span = None

    def close_verse():
        nonlocal verse_span
        if verse_span and cur:
            verse_span[1] = len(cur.text.encode())
            cur.spans.append(verse_span)
        verse_span = None

    for line in open(fetch("tahot.txt"), encoding="utf-8"):
        if not re.match(r"Gen\.\d", line):
            continue
        cols = line.rstrip("\n").split("\t")
        ref = cols[0]
        m = re.match(r"Gen\.(\d+)\.(\d+)(?:\((\d+)\.(\d+)\))?#(\d+)", ref)
        if not m:
            continue
        ec, ev = int(m.group(1)), int(m.group(2))
        hc, hv = (int(m.group(3)), int(m.group(4))) if m.group(3) else (ec, ev)
        heb, translit, gloss, dstrong, grammar = cols[1], cols[2], cols[3], cols[4], cols[5]
        expanded = cols[11] if len(cols) > 11 else ""
        if hc != cur_chapter:
            close_verse()
            cur_chapter = hc
            u = Unit("chapter")
            s, e = u.add(f"בְּרֵאשִׁית {hc}")
            u.spans.append([s, e, "chapter", {"c": hc}])
            units.append(u)
            cur = Unit("p")
            units.append(cur)
        std = f"Gen.{ec}.{ev}"
        if std != cur_verse_std:
            close_verse()
            cur_verse_std = std
            if cur.text and not cur.text.endswith(" "):
                cur.add(" ")
            verse_span = [len(cur.text.encode()), None, "verse", {"ref": std, "label": str(hv), "native": f"Gen.{hc}.{hv}"}]
        surface = heb.replace("/", "")
        para_break = False
        mm = re.search(r"\\ \\([פס])$", surface)
        if mm:
            para_break = True
            surface = surface[: mm.start()] + " " + mm.group(1)
        surface = nfc(surface.replace("\\", ""))
        # main lemma = the {braced} part of the expanded tags
        lm = re.search(r"\{(H\d+[A-Z]?)=([^=]+)=([^}]*)\}", expanded)
        lemma, lstrong, lgloss = (lm.group(2), lm.group(1), lm.group(3)) if lm else ("", "", "")
        lgloss = lgloss.split("»")[-1].split(":")[0].replace("_", " ").strip(" :") if lgloss else ""
        if lgloss.startswith(": "):
            lgloss = lgloss[2:]
        idx = len(words)
        words.append({
            "heb": surface, "translit": translit, "gloss": gloss, "strongs": dstrong,
            "lemma": nfc(lemma), "lemma_strong": lstrong, "lemma_gloss": lgloss,
            "morph": grammar, "morph_en": morph_en(grammar), "ref": f"Gen.{hc}.{hv}",
        })
        if lstrong:
            base = re.sub(r"[A-Z]$", "", lstrong)
            usage[base][(lstrong, nfc(lemma), lgloss)] += 1
        word_text = surface
        maqqef = word_text.endswith("־")
        trailing = ""
        if para_break:
            word_text, trailing = word_text[:-2], word_text[-2:]
        s, e = cur.add(word_text)
        cur.spans.append([s, e, "w", {"i": idx}])
        if trailing:
            cur.add(trailing)
        if not maqqef:
            cur.add(" ")
        if para_break:
            close_verse()
            cur.text = cur.text.rstrip()
            cur = Unit("p")
            units.append(cur)
            cur_verse_std = None
    close_verse()
    units = [u for u in units if u.text.strip()]
    for u in units:
        u.text = u.text.rstrip()
        n = len(u.text.encode())
        for sp in u.spans:
            sp[0], sp[1] = min(sp[0], n), min(sp[1], n)
    for base, c in usage.items():
        (dstr, lemma, g), _ = c.most_common(1)[0]
        lex[base] = {"strong": dstr, "lemma": lemma, "gloss": g, "count": sum(c.values())}
    out = {
        "id": "tahot", "abbrev": "TAHOT", "title": "Translators Amalgamated Hebrew OT (STEPBible)",
        "type": "bible", "lang": "he", "dir": "rtl",
        "attribution": "STEPBible TAHOT, CC BY 4.0, STEP Bible (www.STEPBible.org). Genesis only; morpheme separators removed; grammar codes expanded.",
        "units": [u.json() for u in units], "words": words,
    }
    json.dump(out, open(os.path.join(DATA, "tahot.json"), "w"), ensure_ascii=False)
    json.dump(lex, open(os.path.join(DATA, "lexicon.json"), "w"), ensure_ascii=False)
    print("tahot words", len(words), "lexicon", len(lex))


# ---------------------------------------------------------------- SWORD zCom4 commentaries
GEN_CHAPTER_VERSES = [31, 25, 24, 26, 32, 22, 24, 22, 29, 32, 32, 20, 18, 24, 21, 16, 27, 33, 38, 18, 34, 24, 20, 67, 34, 35, 46, 22, 35, 43, 55, 32, 20, 31, 29, 43, 36, 30, 23, 23, 57, 38, 34, 34, 28, 34, 31, 22, 33, 26]


def sword_gen_entries(zip_name, mod, zcom4):
    z = zipfile.ZipFile(fetch(zip_name))
    base = f"modules/comments/zcom/{mod}/ot"
    bzv, bzs, bzz = (z.read(base + ext) for ext in (".bzv", ".bzs", ".bzz"))
    cache = {}

    def read(idx):
        if zcom4:
            blk, off, size = struct.unpack_from("<III", bzv, idx * 12)
        else:
            blk, off, size = struct.unpack_from("<IIH", bzv, idx * 10)
        if size == 0:
            return ""
        if blk not in cache:
            o, s, _ = struct.unpack_from("<III", bzs, blk * 12)
            cache[blk] = zlib.decompress(bzz[o:o + s])
        return cache[blk][off:off + size].decode("utf-8", "replace")

    idx = 3  # 0 module, 1 testament, 2 Genesis intro, 3 Gen 1 intro
    for c, nv in enumerate(GEN_CHAPTER_VERSES, 1):
        for v in range(1, nv + 1):
            yield f"Gen.{c}.{v}", read(idx + v)
        idx += nv + 1


def osis_to_units(raw, key, label, last):
    """Very small OSIS -> units converter for commentary entries."""
    units = []
    h = Unit("h2")
    s, e = h.add(label)
    h.spans.append([s, e, "key", {"ref": key, "end": last}])
    units.append(h)
    cur = Unit("p")
    units.append(cur)
    stack = []
    raw = re.sub(r"<div type=\"x-milestone\"[^>]*/>", "", raw)
    for m in re.finditer(r"(<[^>]+>)|([^<]+)", raw):
        tag, txt = m.group(1), m.group(2)
        if txt is not None:
            t = nfc(html.unescape(txt))
            t = re.sub(r"\s+", " ", t)
            if not cur.text and not stack:
                t = t.lstrip()
            if t:
                cur.add(t)
            continue
        name = re.match(r"</?\s*([a-zA-Z]+)", tag)
        name = name.group(1) if name else ""
        closing = tag.startswith("</")
        selfclose = tag.endswith("/>")
        if name in ("div", "lb", "p") and (selfclose or closing):
            if cur.text.strip():
                cur = Unit("p")
                units.append(cur)
            continue
        if name == "title":
            if not closing:
                if cur.text.strip():
                    cur = Unit("p")
                    units.append(cur)
                cur.kind = "h3"
            else:
                cur = Unit("p")
                units.append(cur)
            continue
        if name in ("hi", "reference"):
            if closing:
                if stack:
                    k, s0, d = stack.pop()
                    e0 = cur.pos()
                    if e0 > s0:
                        cur.spans.append([s0, e0, k, d])
            elif not selfclose:
                if name == "hi":
                    ty = re.search(r'type="([^"]+)"', tag)
                    stack.append(["b" if ty and ty.group(1) == "bold" else "i", cur.pos(), None])
                else:
                    r = re.search(r'osisRef="([^"]+)"', tag)
                    stack.append(["ref", cur.pos(), {"ref": r.group(1) if r else ""}])
            continue
    out = []
    for u in units:
        if not u.text.strip():
            continue
        lead = len(u.text) - len(u.text.lstrip())
        if lead:
            shift = len(u.text[:lead].encode())
            u.text = u.text.lstrip()
            for sp in u.spans:
                sp[0] = max(0, sp[0] - shift)
                sp[1] = max(0, sp[1] - shift)
        u.text = u.text.rstrip()
        n = len(u.text.encode())
        for sp in u.spans:
            sp[0], sp[1] = min(sp[0], n), min(sp[1], n)
        out.append(u)
    return out


def commentary(zip_name, mod, rid, abbrev, title, attribution, zcom4):
    groups = []  # [first_key, last_key, raw]
    for key, raw in sword_gen_entries(zip_name, mod, zcom4):
        if not raw.strip():
            continue
        if groups and groups[-1][2] == raw:
            groups[-1][1] = key
            continue
        groups.append([key, key, raw])
    units = []
    for key, last, raw in groups:
        if key == "Gen.1.1" and mod == "jfb":
            # entry 1:1 carries the whole book introduction; keep the verse note only
            i = raw.find("1. In the beginning")
            if i > 0:
                raw = raw[raw.rfind("<", 0, i - 20):]
        _, c, v = key.split(".")
        _, c2, v2 = last.split(".")
        label = f"Genesis {c}:{v}" if last == key else (f"Genesis {c}:{v}-{v2}" if c == c2 else f"Genesis {c}:{v}-{c2}:{v2}")
        units += osis_to_units(raw, key, label, last)
    out = {"id": rid, "abbrev": abbrev, "title": title, "type": "commentary", "lang": "en", "dir": "ltr",
           "attribution": attribution, "units": [u.json() for u in units]}
    json.dump(out, open(os.path.join(DATA, f"{rid}.json"), "w"), ensure_ascii=False)
    print(rid, "units", len(units))


# ---------------------------------------------------------------- cross refs
def xrefs(kjv_plain):
    z = zipfile.ZipFile(fetch("xref.zip"))
    raw = z.read("cross_references.txt").decode()
    by = defaultdict(list)
    for line in raw.split("\n")[1:]:
        cols = line.split("\t")
        if len(cols) < 3 or not cols[0].startswith("Gen."):
            continue
        by[cols[0]].append((cols[1], int(cols[2])))
    out, texts = {}, {}
    for k, lst in by.items():
        lst.sort(key=lambda x: -x[1])
        keep = [x for x in lst if x[1] > 0][:25]
        out[k] = [[t, v] for t, v in keep]
        for t, _ in keep:
            first = t.split("-")[0]
            if first in kjv_plain:
                texts[first] = kjv_plain[first]
    json.dump({"title": "OpenBible.info Cross References", "attribution": "OpenBible.info, CC BY 4.0. Votes as published.",
               "refs": out, "kjv_text": texts}, open(os.path.join(DATA, "xref.json"), "w"), ensure_ascii=False)
    print("xref verses", len(out), "target texts", len(texts))


def fonts():
    z = zipfile.ZipFile(fetch("gentium.zip"))
    for f in ["GentiumPlus-Regular.ttf", "GentiumPlus-Italic.ttf", "GentiumPlus-Bold.ttf", "OFL.txt"]:
        data = z.read(f"GentiumPlus-6.200/{f}")
        open(os.path.join(FONTS, f if f != "OFL.txt" else "OFL-GentiumPlus.txt"), "wb").write(data)
    z = zipfile.ZipFile(fetch("ezra.zip"))
    open(os.path.join(FONTS, "SILEOT.ttf"), "wb").write(z.read("EzraSIL2.51/SILEOT.ttf"))
    open(os.path.join(FONTS, "OFL-EzraSIL.txt"), "wb").write(z.read("EzraSIL2.51/Licenses.txt"))


if __name__ == "__main__":
    fonts()
    plain = kjv()
    tahot()
    commentary("JFB.zip", "jfb", "jfb", "JFB", "Jamieson-Fausset-Brown Commentary (1871)",
               "Public Domain. CrossWire JFB SWORD module.", True)
    commentary("MHCC.zip", "mhcc", "mhcc", "MHCC", "Matthew Henry's Concise Commentary",
               "Public Domain. CrossWire MHCC SWORD module.", False)
    xrefs(plain)
    for f in sorted(os.listdir(DATA)):
        print(f, os.path.getsize(os.path.join(DATA, f)))
