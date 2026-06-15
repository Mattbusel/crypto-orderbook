"""
Crypto Order Book Engine — Living Documentation Generator
Run this any time to regenerate the PDF with the latest content.
"""

from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import getSampleStyleSheet, ParagraphStyle
from reportlab.lib.units import mm
from reportlab.lib import colors
from reportlab.platypus import (
    SimpleDocTemplate, Paragraph, Spacer, HRFlowable,
    Table, TableStyle, PageBreak, KeepTogether
)
from reportlab.graphics.shapes import (
    Drawing, Rect, String, Line, Polygon,
    Circle, Group
)
from reportlab.graphics import renderPDF
from reportlab.lib.enums import TA_CENTER, TA_LEFT, TA_RIGHT
import os

# ── Output path ────────────────────────────────────────────────────────────────
OUT_PATH = os.path.join(os.path.dirname(__file__), "crypto_orderbook_design.pdf")

# ── Color palette ──────────────────────────────────────────────────────────────
C_BG        = colors.HexColor("#0D1117")   # deep navy — page feel
C_RUST      = colors.HexColor("#CE422B")   # Rust orange-red
C_ACCENT    = colors.HexColor("#58A6FF")   # bright blue
C_GREEN     = colors.HexColor("#3FB950")   # success green
C_YELLOW    = colors.HexColor("#D29922")   # warning amber
C_MUTED     = colors.HexColor("#8B949E")   # muted text
C_WHITE     = colors.HexColor("#E6EDF3")   # near-white
C_DARK      = colors.HexColor("#161B22")   # card background
C_BORDER    = colors.HexColor("#30363D")   # border gray

W, H = A4  # 210 x 297 mm

# ── Styles ─────────────────────────────────────────────────────────────────────
base = getSampleStyleSheet()

def S(name, **kw):
    return ParagraphStyle(name, **kw)

styles = {
    "title": S("title",
        fontName="Helvetica-Bold", fontSize=32, textColor=C_WHITE,
        spaceAfter=6, leading=38),
    "subtitle": S("subtitle",
        fontName="Helvetica", fontSize=14, textColor=C_ACCENT,
        spaceAfter=20, leading=18),
    "h1": S("h1",
        fontName="Helvetica-Bold", fontSize=20, textColor=C_RUST,
        spaceBefore=18, spaceAfter=8, leading=24),
    "h2": S("h2",
        fontName="Helvetica-Bold", fontSize=14, textColor=C_ACCENT,
        spaceBefore=12, spaceAfter=6, leading=18),
    "h3": S("h3",
        fontName="Helvetica-Bold", fontSize=11, textColor=C_YELLOW,
        spaceBefore=8, spaceAfter=4, leading=14),
    "body": S("body",
        fontName="Helvetica", fontSize=10, textColor=C_WHITE,
        spaceAfter=8, leading=16),
    "body_muted": S("body_muted",
        fontName="Helvetica", fontSize=9, textColor=C_MUTED,
        spaceAfter=6, leading=14),
    "code": S("code",
        fontName="Courier", fontSize=9, textColor=C_GREEN,
        spaceAfter=4, leading=13, backColor=C_DARK,
        leftIndent=10, rightIndent=10),
    "decision": S("decision",
        fontName="Helvetica-Bold", fontSize=10, textColor=C_YELLOW,
        spaceAfter=2, leading=14),
    "caption": S("caption",
        fontName="Helvetica-Oblique", fontSize=8, textColor=C_MUTED,
        alignment=TA_CENTER, spaceAfter=10),
    "tradeoff_we": S("tradeoff_we",
        fontName="Helvetica-Bold", fontSize=9, textColor=C_GREEN, leading=13),
    "tradeoff_alt": S("tradeoff_alt",
        fontName="Helvetica", fontSize=9, textColor=C_MUTED, leading=13),
}

def P(text, style="body"):
    return Paragraph(text, styles[style])

def HR():
    return HRFlowable(width="100%", thickness=0.5, color=C_BORDER, spaceAfter=8)

def Space(n=6):
    return Spacer(1, n)

# ── Tradeoff table ─────────────────────────────────────────────────────────────
def tradeoff_table(rows):
    """rows = [(decision, we_chose, alternative, why)]"""
    header = ["Decision", "We Chose", "Alternative", "Why This One"]
    data = [header] + rows
    col_w = [38*mm, 32*mm, 32*mm, 68*mm]
    t = Table(data, colWidths=col_w)
    t.setStyle(TableStyle([
        ("BACKGROUND",   (0,0), (-1,0), C_RUST),
        ("TEXTCOLOR",    (0,0), (-1,0), C_WHITE),
        ("FONTNAME",     (0,0), (-1,0), "Helvetica-Bold"),
        ("FONTSIZE",     (0,0), (-1,0), 9),
        ("BACKGROUND",   (0,1), (-1,-1), C_DARK),
        ("TEXTCOLOR",    (0,1), (-1,-1), C_WHITE),
        ("FONTNAME",     (0,1), (-1,-1), "Helvetica"),
        ("FONTSIZE",     (0,1), (-1,-1), 8),
        ("ROWBACKGROUNDS",(0,1),(-1,-1),[C_DARK, colors.HexColor("#1C2128")]),
        ("GRID",         (0,0), (-1,-1), 0.5, C_BORDER),
        ("VALIGN",       (0,0), (-1,-1), "TOP"),
        ("TOPPADDING",   (0,0), (-1,-1), 5),
        ("BOTTOMPADDING",(0,0), (-1,-1), 5),
        ("LEFTPADDING",  (0,0), (-1,-1), 6),
        ("RIGHTPADDING", (0,0), (-1,-1), 6),
    ]))
    return t


# ══════════════════════════════════════════════════════════════════════════════
# DIAGRAMS
# ══════════════════════════════════════════════════════════════════════════════

def diagram_system_overview():
    """High-level data flow: Binance → WS Client → Manager → Book → API"""
    d = Drawing(W - 60*mm, 90*mm)
    dw, dh = W - 60*mm, 90*mm

    boxes = [
        ("Binance\nExchange",  20,  35, C_RUST),
        ("WebSocket\nClient",  95,  35, C_ACCENT),
        ("Book\nManager",     170,  35, C_YELLOW),
        ("Order\nBook",       245,  35, C_GREEN),
        ("REST API\n(Axum)",  320,  35, colors.HexColor("#BC8CFF")),
    ]

    box_w, box_h = 60, 40

    for label, x, y, color in boxes:
        d.add(Rect(x, y, box_w, box_h, rx=6, ry=6,
                   fillColor=color, strokeColor=C_WHITE, strokeWidth=1))
        for i, line in enumerate(label.split("\n")):
            d.add(String(x + box_w/2, y + box_h/2 + (6 if len(label.split('\n'))>1 else 0) - i*13,
                         line, fontName="Helvetica-Bold", fontSize=9,
                         fillColor=C_BG, textAnchor="middle"))

    arrows = [
        (80, 55, 95, 55, "WebSocket\nframes"),
        (155, 55, 170, 55, "WsEvent\nchannel"),
        (230, 55, 245, 55, "writes\nbook"),
        (305, 55, 320, 55, "reads via\nRwLock"),
    ]
    for x1, y1, x2, y2, label in arrows:
        d.add(Line(x1, y1, x2, y2, strokeColor=C_MUTED, strokeWidth=1.5))
        d.add(Polygon([x2, y2, x2-6, y2+4, x2-6, y2-4],
                       fillColor=C_MUTED, strokeColor=C_MUTED))
        for i, ln in enumerate(label.split("\n")):
            d.add(String((x1+x2)/2, y1 + 12 + i*10, ln,
                          fontName="Helvetica", fontSize=7,
                          fillColor=C_MUTED, textAnchor="middle"))

    d.add(String(dw/2, 8, "Figure 1 — Top-level data flow",
                  fontName="Helvetica-Oblique", fontSize=8,
                  fillColor=C_MUTED, textAnchor="middle"))
    return d


def diagram_reconnect_backoff():
    """Timeline showing exponential backoff with jitter"""
    d = Drawing(W - 60*mm, 80*mm)
    dw = W - 60*mm

    d.add(String(dw/2, 68, "Reconnect Backoff Timeline",
                  fontName="Helvetica-Bold", fontSize=10,
                  fillColor=C_WHITE, textAnchor="middle"))

    attempts = [
        ("Attempt 1", 0,  "FAIL", 1.0,  "wait ~1s"),
        ("Attempt 2", 60, "FAIL", 2.0,  "wait ~2s"),
        ("Attempt 3", 130,"FAIL", 4.0,  "wait ~4s"),
        ("Attempt 4", 220,"OK",   None,  "connected!"),
    ]

    y_line = 38
    d.add(Line(20, y_line, dw-20, y_line, strokeColor=C_BORDER, strokeWidth=1))

    for label, x, status, wait, note in attempts:
        color = C_GREEN if status == "OK" else C_RUST
        d.add(Circle(x+20, y_line, 8, fillColor=color, strokeColor=C_WHITE, strokeWidth=1))
        d.add(String(x+20, y_line-3, status[0],
                      fontName="Helvetica-Bold", fontSize=7,
                      fillColor=C_WHITE, textAnchor="middle"))
        d.add(String(x+20, y_line+14, label,
                      fontName="Helvetica", fontSize=7,
                      fillColor=C_MUTED, textAnchor="middle"))
        if wait:
            d.add(String(x+20, y_line-18, note,
                          fontName="Helvetica", fontSize=7,
                          fillColor=C_YELLOW, textAnchor="middle"))

    d.add(String(dw/2, 4, "Figure 2 — Each failure doubles the wait. Jitter desynchronizes simultaneous reconnects.",
                  fontName="Helvetica-Oblique", fontSize=7,
                  fillColor=C_MUTED, textAnchor="middle"))
    return d


def diagram_channel_architecture():
    """Shows the mpsc channel and RwLock separation"""
    d = Drawing(W - 60*mm, 100*mm)
    dw = W - 60*mm

    d.add(String(dw/2, 90, "Channel Architecture — Who Owns What",
                  fontName="Helvetica-Bold", fontSize=10,
                  fillColor=C_WHITE, textAnchor="middle"))

    # WS Task box
    d.add(Rect(10, 50, 90, 30, rx=4, ry=4, fillColor=C_DARK, strokeColor=C_ACCENT, strokeWidth=1.5))
    d.add(String(55, 70, "WebSocket Task", fontName="Helvetica-Bold", fontSize=8, fillColor=C_ACCENT, textAnchor="middle"))
    d.add(String(55, 58, "owns connection", fontName="Helvetica", fontSize=7, fillColor=C_MUTED, textAnchor="middle"))

    # Channel
    d.add(Rect(115, 57, 60, 16, rx=3, ry=3, fillColor=colors.HexColor("#1C2128"), strokeColor=C_YELLOW, strokeWidth=1))
    d.add(String(145, 63, "mpsc channel", fontName="Helvetica", fontSize=7, fillColor=C_YELLOW, textAnchor="middle"))

    # Arrow ws→channel
    d.add(Line(100, 65, 115, 65, strokeColor=C_MUTED, strokeWidth=1.5))
    d.add(Polygon([115,65,109,68,109,62], fillColor=C_MUTED, strokeColor=C_MUTED))

    # Manager box
    d.add(Rect(190, 50, 90, 30, rx=4, ry=4, fillColor=C_DARK, strokeColor=C_YELLOW, strokeWidth=1.5))
    d.add(String(235, 70, "Manager Task", fontName="Helvetica-Bold", fontSize=8, fillColor=C_YELLOW, textAnchor="middle"))
    d.add(String(235, 58, "drives state machine", fontName="Helvetica", fontSize=7, fillColor=C_MUTED, textAnchor="middle"))

    # Arrow channel→manager
    d.add(Line(175, 65, 190, 65, strokeColor=C_MUTED, strokeWidth=1.5))
    d.add(Polygon([190,65,184,68,184,62], fillColor=C_MUTED, strokeColor=C_MUTED))

    # RwLock
    d.add(Rect(295, 57, 55, 16, rx=3, ry=3, fillColor=colors.HexColor("#1C2128"), strokeColor=C_GREEN, strokeWidth=1))
    d.add(String(322, 63, "Arc<RwLock>", fontName="Helvetica", fontSize=7, fillColor=C_GREEN, textAnchor="middle"))

    # Arrow manager→rwlock
    d.add(Line(280, 65, 295, 65, strokeColor=C_MUTED, strokeWidth=1.5))
    d.add(Polygon([295,65,289,68,289,62], fillColor=C_MUTED, strokeColor=C_MUTED))

    # Order Book
    d.add(Rect(363, 50, 70, 30, rx=4, ry=4, fillColor=C_DARK, strokeColor=C_GREEN, strokeWidth=1.5))
    d.add(String(398, 70, "Order Book", fontName="Helvetica-Bold", fontSize=8, fillColor=C_GREEN, textAnchor="middle"))
    d.add(String(398, 58, "BTreeMap<price,qty>", fontName="Courier", fontSize=6, fillColor=C_MUTED, textAnchor="middle"))

    # API readers (below)
    d.add(Rect(295, 15, 138, 22, rx=4, ry=4, fillColor=C_DARK, strokeColor=colors.HexColor("#BC8CFF"), strokeWidth=1))
    d.add(String(364, 24, "Axum API handlers — read-only, many simultaneous readers OK",
                  fontName="Helvetica", fontSize=7, fillColor=colors.HexColor("#BC8CFF"), textAnchor="middle"))

    # Arrow book→api
    d.add(Line(398, 50, 398, 37, strokeColor=C_MUTED, strokeWidth=1))
    d.add(Polygon([398,37,395,43,401,43], fillColor=C_MUTED, strokeColor=C_MUTED))

    d.add(String(dw/2, 4, "Figure 3 — One writer (manager), many readers (API). The channel enforces ordering.",
                  fontName="Helvetica-Oblique", fontSize=7,
                  fillColor=C_MUTED, textAnchor="middle"))
    return d


def diagram_error_taxonomy():
    """Visual map of error types to recovery actions"""
    d = Drawing(W - 60*mm, 110*mm)
    dw = W - 60*mm

    d.add(String(dw/2, 100, "Error Types and What Actually Happens",
                  fontName="Helvetica-Bold", fontSize=10,
                  fillColor=C_WHITE, textAnchor="middle"))

    errors = [
        ("WebSocket",    "Network dropped",          "Reconnect",           C_RUST,   10,  72),
        ("Parse",        "Bad JSON from Binance",    "Skip frame, keep going", C_YELLOW, 10, 50),
        ("SequenceGap",  "Missed an update",         "Rebuild book from scratch", C_ACCENT, 10, 28),
        ("SyncFailed",   "Bad initial handshake",    "Redo handshake",      C_MUTED,  10,  6),
        ("InvalidUrl",   "Bad config at startup",    "Crash immediately",   C_RUST,  260, 72),
        ("Io",           "OS-level network error",   "Reconnect",           C_RUST,  260, 50),
    ]

    for name, what, action, color, x, y in errors:
        d.add(Rect(x, y, 170, 20, rx=3, ry=3, fillColor=C_DARK, strokeColor=color, strokeWidth=1))
        d.add(String(x+6, y+13, name, fontName="Helvetica-Bold", fontSize=8, fillColor=color))
        d.add(String(x+6, y+4,  f"{what}  →  {action}", fontName="Helvetica", fontSize=7, fillColor=C_MUTED))

    d.add(String(dw/2, 0, "Figure 4 — Each error variant maps to a specific, different recovery action.",
                  fontName="Helvetica-Oblique", fontSize=7,
                  fillColor=C_MUTED, textAnchor="middle"))
    return d


def diagram_wsevent():
    """Shows the two WsEvent variants flowing through the channel"""
    d = Drawing(W - 60*mm, 70*mm)
    dw = W - 60*mm

    d.add(String(dw/2, 62, "What Flows Through the Channel",
                  fontName="Helvetica-Bold", fontSize=10,
                  fillColor=C_WHITE, textAnchor="middle"))

    # Message variant
    d.add(Rect(10, 35, 180, 22, rx=3, ry=3, fillColor=C_DARK, strokeColor=C_GREEN, strokeWidth=1.5))
    d.add(String(16, 50, "Message(String)", fontName="Courier-Bold", fontSize=9, fillColor=C_GREEN))
    d.add(String(16, 39, 'Raw JSON: {"e":"depthUpdate","U":123456,...}', fontName="Courier", fontSize=7, fillColor=C_MUTED))

    # Reconnected variant
    d.add(Rect(10, 8, 180, 22, rx=3, ry=3, fillColor=C_DARK, strokeColor=C_YELLOW, strokeWidth=1.5))
    d.add(String(16, 23, "Reconnected", fontName="Courier-Bold", fontSize=9, fillColor=C_YELLOW))
    d.add(String(16, 12, "No data. Signal only. Manager must resync.", fontName="Helvetica", fontSize=7, fillColor=C_MUTED))

    # Arrow → Manager
    d.add(Line(190, 46, 240, 46, strokeColor=C_MUTED, strokeWidth=1.5))
    d.add(Line(190, 19, 240, 19, strokeColor=C_MUTED, strokeWidth=1.5))
    d.add(Polygon([240,46,234,49,234,43], fillColor=C_MUTED, strokeColor=C_MUTED))
    d.add(Polygon([240,19,234,22,234,16], fillColor=C_MUTED, strokeColor=C_MUTED))

    d.add(Rect(240, 10, 110, 44, rx=4, ry=4, fillColor=C_DARK, strokeColor=C_ACCENT, strokeWidth=1.5))
    d.add(String(295, 40, "Manager", fontName="Helvetica-Bold", fontSize=9, fillColor=C_ACCENT, textAnchor="middle"))
    d.add(String(295, 28, "apply update", fontName="Helvetica", fontSize=8, fillColor=C_GREEN, textAnchor="middle"))
    d.add(String(295, 17, "discard + resync", fontName="Helvetica", fontSize=8, fillColor=C_YELLOW, textAnchor="middle"))

    d.add(String(dw/2, 2, "Figure 5 — Two variants, two completely different manager behaviors.",
                  fontName="Helvetica-Oblique", fontSize=7,
                  fillColor=C_MUTED, textAnchor="middle"))
    return d


# ══════════════════════════════════════════════════════════════════════════════
# PAGE BACKGROUND
# ══════════════════════════════════════════════════════════════════════════════

def on_page(canvas, doc):
    canvas.saveState()
    canvas.setFillColor(C_BG)
    canvas.rect(0, 0, W, H, fill=1, stroke=0)

    # Bottom strip
    canvas.setFillColor(C_DARK)
    canvas.rect(0, 0, W, 18*mm, fill=1, stroke=0)
    canvas.setFillColor(C_MUTED)
    canvas.setFont("Helvetica", 7)
    canvas.drawString(20*mm, 7*mm, "Crypto Order Book Engine — System Design Document")
    canvas.drawRightString(W - 20*mm, 7*mm, f"Page {doc.page}")

    canvas.restoreState()


# ══════════════════════════════════════════════════════════════════════════════
# CONTENT
# ══════════════════════════════════════════════════════════════════════════════

def build_content():
    story = []

    # ── Cover ──────────────────────────────────────────────────────────────────
    story += [
        Space(40),
        P("Crypto Order Book Engine", "title"),
        P("System Design & Architecture Documentation", "subtitle"),
        HR(),
        P("A production-grade, real-time order book built in Rust. "
          "WebSocket ingestion from Binance, async Tokio throughout, "
          "REST API with Axum. Every decision documented. Every tradeoff named.", "body"),
        Space(8),
        P("Language: Rust  |  Runtime: Tokio  |  Exchange: Binance  |  API: Axum", "body_muted"),
        PageBreak(),
    ]

    # ── Part 1: What Are We Building ──────────────────────────────────────────
    story += [
        P("Part 1 — What Are We Actually Building", "h1"),
        HR(),
        P("Binance is a crypto exchange. When people buy and sell Bitcoin, those "
          "orders sit in a list called an <b>order book</b> — bids on one side "
          "(people wanting to buy), asks on the other (people wanting to sell). "
          "That list changes thousands of times per second.", "body"),
        P("We are building a program that does three things:", "body"),
        P("1.  Connects to Binance and receives those changes in real time.", "body"),
        P("2.  Keeps its own copy of the order book in memory, always up to date.", "body"),
        P("3.  Lets other programs ask what the order book looks like right now, over HTTP.", "body"),
        Space(6),
        P("Simple goal. Hard to do correctly at production quality. "
          "This document explains every decision we made and exactly why we made it.", "body"),
        Space(10),
        diagram_system_overview(),
        Space(10),
    ]

    # ── Part 2: Dependencies ───────────────────────────────────────────────────
    story += [
        P("Part 2 — Cargo.toml: The Dependency Decisions", "h1"),
        HR(),
        P("Every dependency is a bet — you're betting this tool is mature, maintained, "
          "and won't fight with the rest of your system. Here is every bet we made and why.", "body"),
        Space(8),

        P("tokio", "h2"),
        P("Rust by default does one thing at a time. If you're waiting for Binance to send you "
          "a message, your program just sits there doing nothing. Tokio is the system that lets "
          "your program do many things at the same time — listen to Binance, handle an incoming "
          "HTTP request, run a timer — without actually needing multiple CPU cores. "
          "It is the foundation everything else sits on. Without it, none of the other choices make sense.", "body"),

        P("tokio-tungstenite", "h2"),
        P("Binance sends us data over a WebSocket. A WebSocket is like a phone call — you open "
          "the connection once and both sides can talk back and forth as long as they want, "
          "instead of making a new request every time like normal web traffic. "
          "tokio-tungstenite speaks that protocol. We picked this one specifically because it is "
          "built to work with Tokio — they use the same underlying machinery so they do not fight each other.", "body"),

        P("futures-util", "h2"),
        P("When Binance sends us a message over the WebSocket connection, it does not arrive all "
          "at once with a neat label on it. It arrives as a stream — like water from a tap, not a "
          "package being delivered. futures-util gives us the tools to work with that stream: "
          "read the next item, process it, wait for the next one.", "body"),

        P("serde and serde_json", "h2"),
        P('Binance sends us JSON. It looks like this:', "body"),
        P('{"e":"depthUpdate","E":1699000000000,"s":"BTCUSDT","U":123456,"u":123460,...}', "code"),
        P("We need to turn that text into actual Rust values we can work with. "
          "serde is the system that does that conversion automatically. "
          "You describe the shape of what you expect, it reads the JSON and fills in the values. "
          "This is the Rust standard — every serious Rust project uses it.", "body"),

        P("rust_decimal", "h2"),
        P("This one is non-negotiable. Binance sends prices as strings like \"29431.50000000\". "
          "The obvious thing to do is convert that to a floating point number. "
          "But floating point numbers cannot represent most decimal values exactly. "
          "0.1 + 0.2 in floating point does not equal 0.3 — it equals 0.30000000000000004. "
          "In a calculator app that is annoying. In a financial system that is a lawsuit. "
          "rust_decimal stores the number exactly as written. We never use floats for prices, ever.", "body"),

        P("thiserror", "h2"),
        P("When something goes wrong, different things going wrong need different responses. "
          "The network dropped? Reconnect. Got a message we could not parse? Log it and keep going. "
          "The order book got out of sequence? Throw everything away and start over from scratch. "
          "thiserror lets us define a list of named failure types so that when something breaks, "
          "the code that catches it can look at which type it is and react correctly. "
          "Without it, we would have one generic error that tells us nothing about what to do.", "body"),

        P("axum", "h2"),
        P("This is the HTTP server. When another program wants to ask what the current order book "
          "looks like, they send an HTTP request and we send back the answer. Axum handles that. "
          "We picked Axum because it is built by the same team as Tokio and works with it natively, "
          "instead of other options that have their own competing runtime underneath.", "body"),

        P("tracing and tracing-subscriber", "h2"),
        P("In a normal program you would just print to see what is happening. But our program does "
          "many things simultaneously. A regular print statement has no way to tell you which of "
          "those things the message came from. tracing is logging that understands concurrency — "
          "every message knows what task it came from, what connection it was on, "
          "what it was in the middle of doing. When something goes wrong at 3am, "
          "this is how you find out why.", "body"),

        P("url", "h2"),
        P("Instead of building the Binance WebSocket address by gluing strings together, "
          "we use a typed URL that validates the address as we build it. "
          "String concatenation for URLs is how you get subtle bugs — a missing slash, "
          "a wrong character — that only show up at runtime. This catches them at startup.", "body"),

        Space(10),
        P("Tradeoff Summary", "h3"),
        tradeoff_table([
            ["Price type",    "rust_decimal",   "f64 float",      "Floats have rounding errors. Financial systems cannot have silent rounding errors."],
            ["HTTP framework", "axum 0.7",      "actix-web",      "actix-web has its own actor runtime that conflicts with tokio. axum is tokio-native."],
            ["Error handling", "thiserror",     "anyhow",         "anyhow erases error types. We need to match on error variants to know how to recover."],
            ["Logging",       "tracing",        "println / log",  "println has no concept of which async task it came from. tracing tracks task context."],
        ]),
        PageBreak(),
    ]

    # ── Part 3: Error Taxonomy ─────────────────────────────────────────────────
    story += [
        P("Part 3 — error.rs: The Failure Taxonomy", "h1"),
        HR(),
        P("We wrote the error types before any real logic. This is deliberate. "
          "If you define what can go wrong before you write the code, every piece of code "
          "knows exactly what failures it is allowed to produce. "
          "If you do it the other way around, you end up with scattered, incoherent errors "
          "that have no relationship to each other.", "body"),
        Space(8),
        diagram_error_taxonomy(),
        Space(10),

        P("WebSocket", "h2"),
        P("The phone call with Binance dropped. Maybe their server restarted, maybe your internet "
          "hiccupped, maybe a router between you died. "
          "When this happens: hang up and call back. The order book is still valid — "
          "we just need a new connection.", "body"),

        P("Parse", "h2"),
        P("Binance sent us a message we could not understand. Maybe our code has a bug in how "
          "we described the expected shape. Maybe Binance sent something unusual. "
          "When this happens: log the bad message, throw it away, keep the connection open, keep going. "
          "One bad frame does not invalidate the entire stream.", "body"),

        P("SequenceGap", "h2"),
        P("Binance does not send us the full order book every time — that would be too much data. "
          "Instead they send changes: remove this price level, add this one, update that one. "
          "Each change has a sequence number so we can tell if we missed any. "
          "If we get number 100 then number 102 without ever seeing 101, "
          "we have lost a change and our book is now wrong — we do not know what 101 was. "
          "When this happens: throw away everything and rebuild the book from scratch.", "body"),

        P("SyncFailed", "h2"),
        P("When we first connect, there is a specific handshake we do with Binance to make sure "
          "our copy of the book and their actual book are starting from the same point. "
          "If that handshake fails — if the numbers do not add up — "
          "we cannot trust anything. When this happens: start the handshake over.", "body"),

        P("InvalidUrl", "h2"),
        P("The address we are trying to connect to is malformed. "
          "This only ever happens at startup if the configuration is wrong. "
          "When this happens: crash immediately with a clear message. "
          "There is no recovering from bad configuration — you need a human to fix it.", "body"),

        P("Io", "h2"),
        P("Low-level network errors from the operating system itself. "
          "Connection refused, address unreachable, that kind of thing. "
          "When this happens: treat it the same as a dropped WebSocket and reconnect.", "body"),

        PageBreak(),
    ]

    # ── Part 4: WsEvent ────────────────────────────────────────────────────────
    story += [
        P("Part 4 — ws/mod.rs: The Channel Contract", "h1"),
        HR(),
        P("The WebSocket client and the order book manager are two separate parts of the system "
          "that run independently and never call each other directly. "
          "They communicate through a queue — the client puts things in, the manager takes things out. "
          "WsEvent defines everything that can ever go in that queue. "
          "It is a contract between two parts of the system that do not otherwise know about each other.", "body"),
        Space(8),
        diagram_wsevent(),
        Space(10),

        P("Message(String)", "h2"),
        P("Binance sent us something. Here is the raw text, exactly as received. "
          "We do not parse it here — that is the manager's job. "
          "The WebSocket client's only responsibility is to get the bytes from Binance to the queue. "
          "Keeping parsing out of the client means if Binance ever changes their JSON format, "
          "we fix it in one place and the client never needs to change.", "body"),

        P("Reconnected", "h2"),
        P("We lost the connection and re-established it. "
          "Whatever the manager knew before is now potentially stale — "
          "the book might be based on a stream that is no longer continuous. "
          "Start over. This signal arrives in the queue before any new messages, "
          "which is critical — if new messages arrived first, the manager would try "
          "to apply them to a book that might be from a completely different session.", "body"),

        PageBreak(),
    ]

    # ── Part 5: WS Client ─────────────────────────────────────────────────────
    story += [
        P("Part 5 — ws/client.rs: The Reconnecting WebSocket Client", "h1"),
        HR(),
        P("This is the piece that actually talks to Binance. "
          "It runs forever in the background, maintaining the connection and "
          "feeding messages into the queue. Here is every decision behind it.", "body"),
        Space(8),
        diagram_channel_architecture(),
        Space(10),

        P("Why it runs in its own independent task", "h2"),
        P("When we call spawn, we hand the connection off to a background process and walk away. "
          "We do not sit there waiting for it to finish — it never finishes, that is the point. "
          "It runs forever in the background, feeding us messages. "
          "If we did not do it this way, the act of listening to Binance would block everything "
          "else in our program from running.", "body"),

        P("The reconnect loop — what actually happens", "h2"),
        P("The outer loop is simple: try to connect. If it works, read messages until it breaks. "
          "When it breaks, try to connect again. "
          "The complexity is in how long we wait between attempts.", "body"),
        Space(8),
        diagram_reconnect_backoff(),
        Space(10),

        P("Why we wait at all — and why the wait grows", "h2"),
        P("Imagine Binance's servers go down. If every client immediately tries to reconnect "
          "and keeps trying every millisecond, Binance's servers come back up and immediately "
          "get flooded by millions of simultaneous reconnect attempts — which might knock them down again. "
          "We wait. And we wait longer each time we fail, because if we have failed three times in a row, "
          "something is genuinely wrong and hammering the server harder is not going to fix it.", "body"),

        P("Why we add randomness to the wait", "h2"),
        P("Even with growing wait times, if ten thousand clients all started at the same moment "
          "they will all be on the same schedule. They all wait 1 second, all try at once, "
          "all fail, all wait 2 seconds, all try at once again. "
          "Adding a small random amount to each client's wait time spreads them out "
          "so they do not all hit at the exact same millisecond.", "body"),

        P("Why we cap the wait at 30 seconds", "h2"),
        P("Pure doubling would eventually mean waiting hours between attempts. "
          "That is too long — if Binance comes back up after 5 minutes we want to reconnect "
          "within 30 seconds, not be stuck waiting another hour. 30 seconds is the ceiling.", "body"),

        P("The safety math — what happens after 64 failures", "h2"),
        P("The wait time formula doubles each time. After 64 failed attempts, "
          "2 to the power of 64 is a number so large it does not fit in the variable storing it. "
          "In most languages that silently wraps around to zero — no wait at all — "
          "exactly the wrong behavior when we are trying to avoid hammering a broken server. "
          "We used arithmetic that, when it would overflow, just stays at the maximum value. "
          "Then we cap it at 30 seconds. Two layers of protection.", "body"),

        P("When we send Reconnected — and why it must be first", "h2"),
        P("When we successfully reconnect after a failure, the very first thing we put in the queue "
          "is Reconnected. Before any messages. "
          "This tells the manager: everything you knew is potentially wrong, start over. "
          "If we sent messages first, the manager would try to apply them to a stale book "
          "from the previous session. The ordering of events in the queue is the guarantee.", "body"),

        P("When we shut down cleanly", "h2"),
        P("We check every time we try to put something in the queue whether the other side "
          "is still listening. If the manager has gone away and nobody is reading from the queue, "
          "we stop. We do not keep running a background task that produces output nobody will ever consume.", "body"),

        Space(10),
        tradeoff_table([
            ["Task model",      "spawn (fire and forget)", "return JoinHandle", "Returning a handle implies the caller manages the lifecycle. This task's lifetime is tied to the channel, not the caller."],
            ["Reconnect signal","WsEvent::Reconnected in same channel", "Separate control channel", "Two channels = possible race where a stale message arrives after the reconnect signal. One channel = strict ordering guarantee."],
            ["Jitter source",   "System clock nanoseconds", "rand crate",      "We only need desynchronization, not cryptographic randomness. Pulling in rand for this one use would be over-engineering."],
            ["Overflow safety", "saturating_mul / saturating_pow", "unchecked", "After 64 failures unchecked math wraps to zero — producing instant retries on a broken server. Saturating clamps to max then we cap at 30s."],
        ]),
    ]

    return story


# ══════════════════════════════════════════════════════════════════════════════
# BUILD PDF
# ══════════════════════════════════════════════════════════════════════════════

def main():
    os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)

    doc = SimpleDocTemplate(
        OUT_PATH,
        pagesize=A4,
        leftMargin=20*mm,
        rightMargin=20*mm,
        topMargin=22*mm,
        bottomMargin=24*mm,
        title="Crypto Order Book Engine — System Design",
        author="Mattbusel",
    )

    story = build_content()
    doc.build(story, onFirstPage=on_page, onLaterPages=on_page)
    print(f"PDF written to: {OUT_PATH}")


if __name__ == "__main__":
    main()
