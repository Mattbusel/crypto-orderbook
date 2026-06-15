"""
Crypto Order Book Engine - Design Document
Complete rewrite: Paragraph-wrapped table cells, bounded diagrams, no overflow.
"""

import os
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import ParagraphStyle
from reportlab.lib.units import mm
from reportlab.lib import colors
from reportlab.platypus import (
    SimpleDocTemplate, Paragraph, Spacer, HRFlowable,
    Table, TableStyle, PageBreak, KeepTogether
)
from reportlab.graphics.shapes import Drawing, Rect, String, Line, Polygon, Circle
from reportlab.lib.enums import TA_CENTER, TA_LEFT, TA_RIGHT

OUT_PATH = os.path.join(os.path.dirname(__file__), "crypto_orderbook_design.pdf")

W, H = A4           # 595 x 842 pt
MARGIN = 18 * mm    # left/right margin in doc
AVAIL  = W - 2 * MARGIN  # ~559 pt usable text width

# ── Colour palette ─────────────────────────────────────────────────────────────
BG      = colors.HexColor("#0D1117")
PANEL   = colors.HexColor("#161B22")
BORDER  = colors.HexColor("#30363D")
RUST    = colors.HexColor("#E05C42")
RUST_DK = colors.HexColor("#5a1e14")
BLUE    = colors.HexColor("#58A6FF")
BLUE_DK = colors.HexColor("#0f2040")
GREEN   = colors.HexColor("#3FB950")
GREEN_DK= colors.HexColor("#0a2010")
YELLOW  = colors.HexColor("#D29922")
YEL_DK  = colors.HexColor("#2a1e00")
PURPLE  = colors.HexColor("#BC8CFF")
PUR_DK  = colors.HexColor("#1a0a30")
WHITE   = colors.HexColor("#E6EDF3")
MUTED   = colors.HexColor("#8B949E")
RED_HL  = colors.HexColor("#FF5555")

# ── Paragraph styles ───────────────────────────────────────────────────────────
def _s(name, **kw):
    return ParagraphStyle(name, **kw)

ST = {
    "title":   _s("T",  fontName="Helvetica-Bold",   fontSize=32, textColor=WHITE,
                        spaceAfter=4,  leading=38),
    "sub":     _s("SB", fontName="Helvetica",         fontSize=14, textColor=BLUE,
                        spaceAfter=20, leading=20),
    "h1":      _s("H1", fontName="Helvetica-Bold",   fontSize=19, textColor=RUST,
                        spaceBefore=22, spaceAfter=8, leading=24),
    "h2":      _s("H2", fontName="Helvetica-Bold",   fontSize=13, textColor=BLUE,
                        spaceBefore=14, spaceAfter=5, leading=18),
    "h3":      _s("H3", fontName="Helvetica-Bold",   fontSize=11, textColor=YELLOW,
                        spaceBefore=10, spaceAfter=4, leading=15),
    "body":    _s("B",  fontName="Helvetica",         fontSize=10, textColor=WHITE,
                        spaceAfter=8,  leading=17),
    "muted":   _s("M",  fontName="Helvetica",         fontSize=9,  textColor=MUTED,
                        spaceAfter=6,  leading=14),
    "code":    _s("C",  fontName="Courier",           fontSize=8,  textColor=GREEN,
                        spaceAfter=4,  leading=13, backColor=PANEL,
                        leftIndent=8,  rightIndent=8, spaceBefore=4),
    "caption": _s("CA", fontName="Helvetica-Oblique", fontSize=8,  textColor=MUTED,
                        spaceAfter=14, leading=12, alignment=TA_CENTER),
    # table cell styles
    "th":      _s("TH", fontName="Helvetica-Bold",   fontSize=9,  textColor=WHITE,
                        leading=13),
    "td":      _s("TD", fontName="Helvetica",         fontSize=9,  textColor=WHITE,
                        leading=13),
    "td_g":    _s("TG", fontName="Helvetica",         fontSize=9,  textColor=GREEN,
                        leading=13),
    "td_m":    _s("TM", fontName="Helvetica",         fontSize=9,  textColor=MUTED,
                        leading=13),
}

def P(txt, s="body"): return Paragraph(txt, ST[s])
def HR(): return HRFlowable(width="100%", thickness=0.5, color=BORDER,
                             spaceAfter=8, spaceBefore=4)
def SP(n=8): return Spacer(1, n)
def CAP(t): return P(t, "caption")

# ── Drawing helpers ────────────────────────────────────────────────────────────
# Rule: NO String ever starts within 4pt of the right edge of its parent Drawing.
# All coordinates are in points from the bottom-left of the Drawing.

def dtext(d, x, y, txt, font="Helvetica", sz=8, color=WHITE, anchor="start", maxch=None):
    """Add a string to Drawing d. maxch clips text to prevent overflow."""
    if maxch:
        txt = txt[:maxch]
    d.add(String(x, y, txt, fontName=font, fontSize=sz, fillColor=color,
                 textAnchor=anchor))

def drect(d, x, y, w, h, fill=PANEL, stroke=BORDER, sw=1):
    r = Rect(x, y, w, h)
    r.fillColor = fill; r.strokeColor = stroke; r.strokeWidth = sw
    d.add(r)

def harrow(d, x1, x2, y, color=MUTED, label="", lsz=7):
    """Horizontal arrow from x1 to x2 at height y."""
    d.add(Line(x1, y, x2, y, strokeColor=color, strokeWidth=1.5))
    # arrowhead
    tip = x2
    pts = [tip, y, tip-7, y+3, tip-7, y-3]
    p = Polygon(pts); p.fillColor = color; p.strokeColor = color; p.strokeWidth = 0
    d.add(p)
    if label:
        mid = (x1 + x2) / 2
        dtext(d, mid, y+4, label, sz=lsz, color=color, anchor="middle")

def varrow(d, x, y1, y2, color=MUTED, label="", lsz=7):
    """Vertical arrow from y1 down to y2."""
    d.add(Line(x, y1, x, y2, strokeColor=color, strokeWidth=1.5))
    tip = y2
    pts = [x, tip, x-3, tip+7, x+3, tip+7]
    p = Polygon(pts); p.fillColor = color; p.strokeColor = color; p.strokeWidth = 0
    d.add(p)
    if label:
        mid = (y1 + y2) / 2
        dtext(d, x+5, mid, label, sz=lsz, color=color)

def label_box(d, x, y, w, h, title, subtitle="", fill=PANEL, tc=WHITE, maxch=28):
    """A titled box. title and subtitle are clipped to maxch chars."""
    drect(d, x, y, w, h, fill=fill)
    dtext(d, x + w/2, y + h - 14, title[:maxch],
          font="Helvetica-Bold", sz=8, color=tc, anchor="middle")
    if subtitle:
        dtext(d, x + w/2, y + h - 26, subtitle[:maxch],
              sz=7, color=MUTED, anchor="middle")

# ── Paragraph-based table (cells wrap) ────────────────────────────────────────
def ptable(header_texts, rows, col_widths, header_fill=RUST):
    """
    Build a Table where every cell is a Paragraph so text wraps properly.
    col_widths: list of widths in points. Sum should be <= AVAIL.
    """
    hstyle = ST["th"]
    dstyle = ST["td"]

    def cell(txt, style=dstyle):
        return Paragraph(str(txt), style)

    data = [[cell(h, hstyle) for h in header_texts]]
    for row in rows:
        data.append([cell(c) for c in row])

    t = Table(data, colWidths=col_widths, repeatRows=1)
    t.setStyle(TableStyle([
        ("BACKGROUND",   (0, 0), (-1,  0), header_fill),
        ("BACKGROUND",   (0, 1), (-1, -1), PANEL),
        ("ROWBACKGROUNDS",(0,1),(-1,-1),   [PANEL, colors.HexColor("#1a2030")]),
        ("GRID",         (0, 0), (-1, -1), 0.5, BORDER),
        ("VALIGN",       (0, 0), (-1, -1), "TOP"),
        ("TOPPADDING",   (0, 0), (-1, -1), 5),
        ("BOTTOMPADDING",(0, 0), (-1, -1), 5),
        ("LEFTPADDING",  (0, 0), (-1, -1), 6),
        ("RIGHTPADDING", (0, 0), (-1, -1), 6),
    ]))
    return t

# ── Page background callback ───────────────────────────────────────────────────
def on_page(canvas, doc):
    canvas.saveState()
    canvas.setFillColor(BG)
    canvas.rect(0, 0, W, H, fill=1, stroke=0)
    canvas.restoreState()

# ══════════════════════════════════════════════════════════════════════════════
#  FIGURES
# ══════════════════════════════════════════════════════════════════════════════

def fig_trading_floor():
    """Physical trading floor mapped to our system."""
    dh = 160
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    bw = (AVAIL - 60) / 3  # box width
    boxes = [
        (0,          "BUYERS",       "shout bids",   BLUE_DK,  BLUE),
        (bw + 30,    "EXCHANGE FLOOR","matches orders", RUST_DK, RUST),
        (2*bw + 60,  "SELLERS",      "shout asks",   GREEN_DK, GREEN),
    ]
    for bx, title, sub, fill, tc in boxes:
        label_box(d, bx, 40, bw, 90, title, sub, fill=fill, tc=tc)

    # arrows
    ax1 = bw; ax2 = bw + 30
    harrow(d, ax1, ax2, 85, color=MUTED)
    harrow(d, bw*2+60, bw+30+bw, 85, color=MUTED)

    dtext(d, AVAIL/2, 12, "In our system: Buyers/Sellers = Binance feed. Floor = our OrderBook. You query the floor.",
          sz=7, color=MUTED, anchor="middle", maxch=90)
    return d

def fig_actors():
    """The 4 actors and their relationships."""
    dh = 170
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    bw = 110; bh = 50; gap = (AVAIL - 4*bw) / 5
    items = [
        (gap,            "WS CLIENT",     "connects to Binance", BLUE_DK,  BLUE),
        (gap*2+bw,       "MANAGER",       "state machine",       RUST_DK,  RUST),
        (gap*3+bw*2,     "ORDER BOOK",    "BTreeMap in RAM",     GREEN_DK, GREEN),
        (gap*4+bw*3,     "HTTP API",      "serves queries",      PUR_DK,   PURPLE),
    ]
    for bx, title, sub, fill, tc in items:
        label_box(d, bx, 80, bw, bh, title, sub, fill=fill, tc=tc)

    # arrows between actors
    for i in range(3):
        x1 = gap*(i+1) + bw*(i+1)
        x2 = gap*(i+2) + bw*(i+1)
        harrow(d, x1, x2, 105, color=MUTED)

    # double arrow between BOOK and API
    x_book = gap*3 + bw*2 + bw
    x_api  = gap*4 + bw*3
    harrow(d, x_book, x_api, 118, color=GREEN, label="Arc<RwLock>")

    dtext(d, AVAIL/2, 60, "mpsc channel", sz=7, color=MUTED, anchor="middle")
    dtext(d, gap*3+bw*2 - 20, 60, "writes", sz=7, color=RUST, anchor="middle")

    dtext(d, AVAIL/2, 12,
          "Each actor runs in its own Tokio task. They share nothing except typed channels and a reference-counted lock.",
          sz=7, color=MUTED, anchor="middle", maxch=95)
    return d

def fig_backoff():
    """Exponential backoff curve."""
    dh = 160
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    ox = 55; oy = 25; pw = AVAIL - ox - 20; ph = 110

    # axes
    d.add(Line(ox, oy, ox, oy+ph, strokeColor=MUTED, strokeWidth=1))
    d.add(Line(ox, oy, ox+pw, oy, strokeColor=MUTED, strokeWidth=1))

    # Y-axis labels
    for sec, label in [(0,"0s"),(30,"10s"),(60,"20s"),(90,"30s+")]:
        yy = oy + sec * (ph/90)
        d.add(Line(ox-3, yy, ox, yy, strokeColor=MUTED, strokeWidth=0.5))
        dtext(d, ox-6, yy-3, label, sz=6, color=MUTED, anchor="end")

    # X-axis labels (attempts)
    for att in range(6):
        xx = ox + att * (pw/5)
        d.add(Line(xx, oy-3, xx, oy, strokeColor=MUTED, strokeWidth=0.5))
        dtext(d, xx, oy-12, f"#{att+1}", sz=6, color=MUTED, anchor="middle")

    # plot backoff values: min(1000*2^n, 30000) ms
    vals = [min(1000 * (2**n), 30000) for n in range(6)]
    pts_x = [ox + i*(pw/5) for i in range(6)]
    pts_y = [oy + (v/30000)*ph for v in vals]

    for i in range(5):
        d.add(Line(pts_x[i], pts_y[i], pts_x[i+1], pts_y[i+1],
                   strokeColor=RUST, strokeWidth=2))
    for i in range(6):
        c = Circle(pts_x[i], pts_y[i], 3)
        c.fillColor = RUST; c.strokeColor = RUST
        d.add(c)
        dtext(d, pts_x[i], pts_y[i]+5, f"{vals[i]//1000}s", sz=6, color=YELLOW, anchor="middle")

    # jitter band
    jitter_h = (1000/30000)*ph
    for i in range(6):
        drect(d, pts_x[i]-1, pts_y[i], 2, jitter_h, fill=BLUE_DK, stroke=None)

    dtext(d, ox+pw/2, oy+ph+8, "Attempt number", sz=7, color=MUTED, anchor="middle")
    dtext(d, 14, oy+ph/2, "Wait", sz=7, color=MUTED, anchor="middle")
    dtext(d, ox+pw-60, pts_y[4]+8, "+ jitter (blue)", sz=6, color=BLUE, maxch=20)
    dtext(d, ox+pw-60, pts_y[4]-2, "caps at 30s", sz=6, color=YELLOW, maxch=15)
    return d

def fig_ws_states():
    """WebSocket client state machine."""
    dh = 180
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    bw = 120; bh = 44
    states = [
        (20,  100, "CONNECTING",   "attempt #N",    BLUE_DK,  BLUE),
        (200, 100, "LIVE",         "forwarding msgs",GREEN_DK, GREEN),
        (380, 100, "BACKING OFF",  "sleeping",       YEL_DK,   YELLOW),
    ]
    for bx, by, title, sub, fill, tc in states:
        label_box(d, bx, by, bw, bh, title, sub, fill=fill, tc=tc)

    # CONNECTING -> LIVE
    harrow(d, 20+bw, 200, 122, color=GREEN, label="connected")
    # LIVE -> BACKING OFF
    harrow(d, 200+bw, 380, 122, color=RUST, label="error/close")
    # BACKING OFF -> CONNECTING
    # draw curve via two segments
    d.add(Line(380+60, 100, 380+60, 68, strokeColor=YELLOW, strokeWidth=1.5))
    d.add(Line(380+60, 68, 80, 68, strokeColor=YELLOW, strokeWidth=1.5))
    varrow(d, 80, 68, 100, color=YELLOW)
    dtext(d, 230, 58, "backoff expires -> retry", sz=7, color=YELLOW, anchor="middle")

    # send Reconnected signal
    dtext(d, 200+bw/2, 90, "sends WsEvent::Reconnected", sz=6, color=RUST, anchor="middle")

    dtext(d, AVAIL/2, 12,
          "The client never gives up. Each reconnect sends a Reconnected event so the Manager knows to resync.",
          sz=7, color=MUTED, anchor="middle", maxch=90)
    return d

def fig_binance_protocol():
    """Swimlane diagram of the Binance sync handshake."""
    dh = 230
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    lane_h = 42
    lanes = [
        (dh - 50,  "Binance WS",    BLUE_DK,  BLUE),
        (dh - 98,  "Binance REST",  RUST_DK,  RUST),
        (dh - 146, "Our Manager",   GREEN_DK, GREEN),
    ]
    for ly, lname, fill, tc in lanes:
        drect(d, 0, ly, 90, lane_h, fill=fill)
        dtext(d, 45, ly + lane_h/2 - 4, lname, sz=7, color=tc, anchor="middle",
              font="Helvetica-Bold")

    # timeline bar
    tx0 = 100; tx1 = AVAIL - 10
    for ly, _, fill, _ in lanes:
        d.add(Line(tx0, ly + lane_h/2, tx1, ly + lane_h/2,
                   strokeColor=BORDER, strokeWidth=0.5))

    # events on timeline
    events = [
        # (x_frac, lane_idx, label, color)
        (0.05, 0, "subscribe", BLUE),
        (0.15, 0, "event #1",  BLUE),
        (0.25, 0, "event #2",  BLUE),
        (0.35, 0, "event #3",  BLUE),
        (0.40, 1, "GET /depth",RUST),
        (0.55, 1, "snap #2",   RUST),
        (0.60, 2, "apply snap",GREEN),
        (0.70, 2, "discard #1",MUTED),
        (0.80, 2, "apply #2",  GREEN),
        (0.90, 2, "apply #3",  GREEN),
    ]
    for frac, lane_i, label, col in events:
        lx = tx0 + frac * (tx1 - tx0)
        ly = lanes[lane_i][0] + lane_h/2
        c2 = Circle(lx, ly, 4)
        c2.fillColor = col; c2.strokeColor = col
        d.add(c2)
        # label alternates above/below to avoid overlap
        offset = 12 if lane_i % 2 == 0 else -16
        dtext(d, lx, ly + offset, label, sz=6, color=col, anchor="middle", maxch=12)

    # vertical dotted line at snap point
    snap_x = tx0 + 0.40 * (tx1 - tx0)
    d.add(Line(snap_x, dh-150, snap_x, dh-50,
               strokeColor=RUST, strokeWidth=0.8, strokeDashArray=[3,3]))
    dtext(d, snap_x, dh-155, "fetch snapshot here", sz=6, color=RUST, anchor="middle")

    dtext(d, AVAIL/2, 8,
          "Events buffer in our mpsc channel while the REST snapshot fetches. We replay buffered events after.",
          sz=7, color=MUTED, anchor="middle", maxch=90)
    return d

def fig_sequence_check():
    """Visual of the sequence number continuity rule."""
    dh = 140
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    ew = 90; eh = 36; gap = 18; y_ev = 70

    # three events
    for i, (title, u_range, col) in enumerate([
        ("Event A", "U=100  u=105", GREEN),
        ("Event B", "U=106  u=110", GREEN),
        ("Event C", "U=112  u=115", RED_HL),
    ]):
        bx = 30 + i * (ew + gap)
        drect(d, bx, y_ev, ew, eh, fill=PANEL, stroke=col, sw=1.5)
        dtext(d, bx + ew/2, y_ev+eh-13, title, font="Helvetica-Bold",
              sz=7, color=col, anchor="middle")
        dtext(d, bx + ew/2, y_ev+6, u_range, sz=6, color=MUTED, anchor="middle")

    # arrows
    for i in range(2):
        x1 = 30 + (i+1) * (ew + gap) - gap
        x2 = 30 + (i+1) * (ew + gap)
        col = GREEN if i == 0 else RED_HL
        harrow(d, x1, x2, y_ev + eh/2, color=col)

    # annotation under C
    cx = 30 + 2*(ew+gap)
    dtext(d, cx + ew/2, y_ev - 14, "GAP! 111 missing", sz=7,
          color=RED_HL, anchor="middle", font="Helvetica-Bold")
    dtext(d, cx + ew/2, y_ev - 26, "-> trigger resync", sz=7,
          color=RUST, anchor="middle")

    # rule box on right
    rx = 30 + 3*(ew+gap) + 10
    if rx + 130 <= AVAIL:
        drect(d, rx, y_ev, 130, eh, fill=YEL_DK, stroke=YELLOW, sw=1)
        dtext(d, rx+65, y_ev+eh-13, "Rule:", font="Helvetica-Bold",
              sz=7, color=YELLOW, anchor="middle")
        dtext(d, rx+65, y_ev+14, "U[n] == u[n-1]+1", sz=7, color=WHITE, anchor="middle")
        dtext(d, rx+65, y_ev+4, "always", sz=7, color=WHITE, anchor="middle")

    dtext(d, AVAIL/2, 12,
          "Like page numbers in a book. If page 111 is missing, you cannot understand page 112.",
          sz=7, color=MUTED, anchor="middle", maxch=80)
    return d

def fig_btree():
    """BTreeMap memory layout for bids and asks."""
    dh = 200
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    mid = AVAIL / 2
    col_w = mid - 20
    row_h = 22
    prices_bid = [50300, 50250, 50200, 50150]
    prices_ask = [50350, 50400, 50450, 50500]
    qtys_bid   = ["1.24", "0.87", "2.10", "0.55"]
    qtys_ask   = ["0.33", "1.90", "0.72", "1.05"]

    dtext(d, mid/2,    185, "BIDS (buyers)",  font="Helvetica-Bold", sz=8, color=GREEN, anchor="middle")
    dtext(d, mid*1.5,  185, "ASKS (sellers)", font="Helvetica-Bold", sz=8, color=RUST,  anchor="middle")

    for i, (pb, pa, qb, qa) in enumerate(zip(prices_bid, prices_ask, qtys_bid, qtys_ask)):
        y = 155 - i * row_h
        is_best = (i == 0)
        bf = colors.HexColor("#0d2a10") if is_best else GREEN_DK
        af = colors.HexColor("#2a0d0a") if is_best else RUST_DK
        bc = GREEN if is_best else colors.HexColor("#1a4020")
        ac = RUST  if is_best else colors.HexColor("#4a1a14")

        # bid row
        drect(d, 10, y, col_w - 10, row_h - 2, fill=bf, stroke=bc, sw=0.8)
        dtext(d, 20, y + 7, f"{pb:,}", sz=8, color=GREEN, font="Courier")
        dtext(d, col_w - 15, y + 7, qb, sz=8, color=MUTED, anchor="end", font="Courier")

        # ask row
        drect(d, mid + 10, y, col_w - 10, row_h - 2, fill=af, stroke=ac, sw=0.8)
        dtext(d, mid + 20, y + 7, f"{pa:,}", sz=8, color=RUST, font="Courier")
        dtext(d, AVAIL - 15, y + 7, qa, sz=8, color=MUTED, anchor="end", font="Courier")

        if is_best:
            dtext(d, col_w/2, y + row_h - 3, "BEST BID", sz=5, color=GREEN, anchor="middle")
            dtext(d, mid + col_w/2, y + row_h - 3, "BEST ASK", sz=5, color=RUST, anchor="middle")

    # spread callout
    spread = 50350 - 50300
    dtext(d, mid, 135 - 0*row_h - 4, f"spread = {spread}", sz=7,
          color=YELLOW, anchor="middle", font="Helvetica-Bold")

    dtext(d, AVAIL/2, 10,
          "BTreeMap keeps prices sorted automatically. best_bid = last entry. best_ask = first entry. O(log n).",
          sz=7, color=MUTED, anchor="middle", maxch=88)
    return d

def fig_zero_qty():
    """Before/after of a zero-quantity deletion - two simple tables."""
    dh = 160
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    col_w = (AVAIL/2) - 30
    titles = ["BEFORE", "AFTER"]
    colors_ = [GREEN, RUST]

    rows_before = [(50300,"1.24"),(50299,"0.87"),(50298,"2.10"),(50297,"0.55")]
    rows_after  = [(50300,"1.24"),(50299,"---  DELETED ---"),(50298,"2.10"),(50297,"0.55")]
    delete_row  = 1  # index of deleted row in after

    for side, (rows, title, col) in enumerate([(rows_before,"BEFORE",GREEN),(rows_after,"AFTER",RUST)]):
        ox = 10 + side * (col_w + 40)
        dtext(d, ox + col_w/2, 148, title, font="Helvetica-Bold", sz=8, color=col, anchor="middle")
        for i, (price, qty) in enumerate(rows):
            ry = 118 - i * 24
            is_del = (side == 1 and i == delete_row)
            fill = colors.HexColor("#3a0a0a") if is_del else PANEL
            stroke = RED_HL if is_del else BORDER
            drect(d, ox, ry, col_w, 20, fill=fill, stroke=stroke, sw=1)
            dtext(d, ox+8, ry+6, str(price), sz=8, color=MUTED if is_del else WHITE, font="Courier")
            qty_col = RED_HL if is_del else MUTED
            qty_str = str(qty)[:20]
            dtext(d, ox + col_w - 8, ry+6, qty_str, sz=8,
                  color=qty_col, anchor="end", font="Courier")

    # Arrow in the middle
    mx = col_w + 25
    harrow(d, mx, mx+16, 106, color=YELLOW, label='qty=0')

    dtext(d, AVAIL/2, 10,
          "Binance does not send a 'delete' command. A quantity of zero IS the delete. Our is_removal() checks for it.",
          sz=7, color=MUTED, anchor="middle", maxch=90)
    return d

def fig_two_layer():
    """Wire format -> domain type two-layer parsing."""
    dh = 150
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    bw = 180; bh = 95
    # left box: wire format
    drect(d, 10, 35, bw, bh, fill=BLUE_DK, stroke=BLUE, sw=1.5)
    dtext(d, 10+bw/2, 35+bh-12, "WIRE FORMAT", font="Helvetica-Bold", sz=8, color=BLUE, anchor="middle")
    dtext(d, 10+bw/2, 35+bh-24, "private to binance.rs", sz=7, color=MUTED, anchor="middle")
    for i, line in enumerate(['RawDepthUpdate {', '  "U","u","b","a": strings', '}']):
        dtext(d, 20, 35+bh-40 - i*12, line, sz=7, color=GREEN, font="Courier", maxch=26)

    # right box: domain type
    rx = AVAIL - 10 - bw
    drect(d, rx, 35, bw, bh, fill=GREEN_DK, stroke=GREEN, sw=1.5)
    dtext(d, rx+bw/2, 35+bh-12, "DOMAIN TYPE", font="Helvetica-Bold", sz=8, color=GREEN, anchor="middle")
    dtext(d, rx+bw/2, 35+bh-24, "used everywhere else", sz=7, color=MUTED, anchor="middle")
    for i, line in enumerate(['DepthUpdate {', '  first/last_update_id: u64', '  bids/asks: Vec<Level>', '}']):
        dtext(d, rx+10, 35+bh-40 - i*12, line, sz=7, color=GREEN, font="Courier", maxch=26)

    # middle arrow with parse label
    ax1 = 10 + bw + 4
    ax2 = rx - 4
    harrow(d, ax1, ax2, 82, color=YELLOW, label="parse() + validate", lsz=7)

    # callout at bottom
    drect(d, 10, 5, AVAIL-20, 24, fill=YEL_DK, stroke=YELLOW, sw=1)
    dtext(d, AVAIL/2, 13, "If Binance renames a field tomorrow: change one line in binance.rs. Nothing else changes.",
          sz=7, color=YELLOW, anchor="middle", maxch=88)
    return d

def fig_manager_fsm():
    """Manager three-phase state machine."""
    dh = 190
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    bw = 130; bh = 50; gap = (AVAIL - 3*bw) / 4
    phases = [
        (gap,         "PHASE 1",   "Fetch Snapshot", RUST_DK,  RUST),
        (gap*2+bw,    "PHASE 2",   "Handshake",      BLUE_DK,  BLUE),
        (gap*3+bw*2,  "PHASE 3",   "Live Mode",      GREEN_DK, GREEN),
    ]
    by = 100
    for bx, ph, label, fill, tc in phases:
        drect(d, bx, by, bw, bh, fill=fill, stroke=tc, sw=1.5)
        dtext(d, bx+bw/2, by+bh-13, ph, font="Helvetica-Bold", sz=8, color=tc, anchor="middle")
        dtext(d, bx+bw/2, by+14, label, sz=7, color=WHITE, anchor="middle")

    # arrows between phases
    for i in range(2):
        x1 = gap*(i+1) + bw*(i+1)
        x2 = gap*(i+2) + bw*(i+1)
        harrow(d, x1, x2, by+bh/2, color=MUTED)

    # error recovery arrows (back to start)
    for i, (bx, _, _, _, tc) in enumerate(phases[1:], 1):
        bx_actual = gap*(i+1) + bw*i
        # arrow down and back
        ydown = by - 15
        d.add(Line(bx_actual + bw/2, by, bx_actual + bw/2, ydown, strokeColor=tc, strokeWidth=1, strokeDashArray=[3,3]))
        d.add(Line(bx_actual + bw/2, ydown, gap + bw/2, ydown, strokeColor=RUST, strokeWidth=1, strokeDashArray=[3,3]))
        varrow(d, gap + bw/2, ydown, by, color=RUST)

    dtext(d, AVAIL/2, 72, "gap / reconnect -> restart from Phase 1", sz=7, color=RUST, anchor="middle")

    # sub-labels
    sub = [
        "REST /api/v3/depth\nwith retry backoff",
        "Discard stale events\nFind first valid",
        "Validate U==prev+1\nApply every update",
    ]
    for i, (bx, _, _, _, tc) in enumerate(phases):
        lines = sub[i].split("\n")
        for j, l in enumerate(lines):
            dtext(d, bx+bw/2, by - 42 - j*12, l, sz=6, color=MUTED, anchor="middle", maxch=24)

    dtext(d, AVAIL/2, 12,
          "The Manager is a state machine. Any failure sends it back to Phase 1. The book is cleared on resync.",
          sz=7, color=MUTED, anchor="middle", maxch=90)
    return d

def fig_arc_rwlock():
    """Arc<RwLock<>> shared memory diagram."""
    dh = 170
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    # central book
    cx = AVAIL/2; cy = 85; bw2 = 130; bh2 = 50
    drect(d, cx - bw2/2, cy - bh2/2, bw2, bh2, fill=GREEN_DK, stroke=GREEN, sw=2)
    dtext(d, cx, cy+10, "ORDER BOOK", font="Helvetica-Bold", sz=9, color=GREEN, anchor="middle")
    dtext(d, cx, cy-4,  "Arc<RwLock<OrderBook>>", sz=7, color=MUTED, anchor="middle")

    # writer: Manager
    wbx = 30; wby = 85
    drect(d, wbx, wby-18, 90, 36, fill=RUST_DK, stroke=RUST, sw=1)
    dtext(d, wbx+45, wby+6, "MANAGER", font="Helvetica-Bold", sz=8, color=RUST, anchor="middle")
    dtext(d, wbx+45, wby-8, "1 writer", sz=7, color=MUTED, anchor="middle")
    harrow(d, wbx+90, cx-bw2/2, wby, color=RUST, label="write lock")

    # readers: API handlers
    readers = [
        (AVAIL-110, 140, "API /best"),
        (AVAIL-110, 85,  "API /snap"),
        (AVAIL-110, 30,  "API /bids"),
    ]
    for rx2, ry, label in readers:
        drect(d, rx2, ry-12, 90, 24, fill=BLUE_DK, stroke=BLUE, sw=1)
        dtext(d, rx2+45, ry+2, label, sz=7, color=BLUE, anchor="middle")
        # arrow to book
        harrow(d, cx+bw2/2, rx2, ry, color=BLUE)

    dtext(d, AVAIL/2, 12,
          "Many readers can hold the lock simultaneously. The writer blocks until all readers finish. This is safe with std::sync::RwLock.",
          sz=7, color=MUTED, anchor="middle", maxch=95)
    return d

def fig_metrics_overview():
    """Prometheus metric types and what we track."""
    dh = 190
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    types_data = [
        ("COUNTER",   BLUE,   "ever-increasing\nnever resets",       ["ws_events_total","updates_applied_total","snapshots_total"]),
        ("GAUGE",     GREEN,  "current value\ncan go up or down",    ["bid_depth","ask_depth","spread","vwap_1m"]),
        ("HISTOGRAM", PURPLE, "distribution\nof observations",       ["http_request_duration"]),
    ]
    bw3 = (AVAIL - 40) / 3; bh3 = 140
    for i, (name, col, desc, examples) in enumerate(types_data):
        bx3 = 10 + i*(bw3+10)
        drect(d, bx3, 30, bw3, bh3, fill=PANEL, stroke=col, sw=1.5)
        dtext(d, bx3+bw3/2, 30+bh3-12, name, font="Helvetica-Bold", sz=8, color=col, anchor="middle")
        for j, dl in enumerate(desc.split("\n")):
            dtext(d, bx3+bw3/2, 30+bh3-28-j*11, dl, sz=6, color=MUTED, anchor="middle", maxch=int(bw3//5))
        for k, ex in enumerate(examples):
            dtext(d, bx3+6, 30+bh3-55-k*14, ex[:int(bw3//5.5)], sz=6, color=col, font="Courier")

    dtext(d, AVAIL/2, 12,
          "Prometheus scrapes /metrics every 15s. Grafana graphs it. Alerts fire when values cross thresholds.",
          sz=7, color=MUTED, anchor="middle", maxch=88)
    return d

def fig_vwap():
    """Rolling VWAP window visualization."""
    dh = 170
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    # timeline bar
    tw = AVAIL - 60; ty = 95; tx0 = 30
    drect(d, tx0, ty - 6, tw, 12, fill=PANEL, stroke=BORDER, sw=0.5)

    # window boundary
    win_start = tx0 + tw * 0.25
    win_end   = tx0 + tw
    drect(d, win_start, ty-25, win_end - win_start, 50,
          fill=colors.HexColor("#102030"), stroke=BLUE, sw=1)
    dtext(d, (win_start + win_end)/2, ty+30, "1-MINUTE WINDOW", sz=7, color=BLUE, anchor="middle")

    # trades as circles with size ~ qty
    trades = [
        (0.05, 50200, 0.5), (0.12, 50350, 1.2), (0.20, 50100, 0.3),
        (0.30, 50280, 2.1), (0.45, 50320, 0.8), (0.60, 50410, 1.5),
        (0.75, 50290, 0.6), (0.90, 50360, 1.9),
    ]
    price_min = 50000; price_range = 500; ph3 = 50

    for frac, price, qty in trades:
        tx3 = tx0 + frac * tw
        py  = ty + (price - price_min) / price_range * ph3 - ph3/2
        r   = max(3, min(8, qty * 3))
        in_window = tx3 >= win_start
        col = BLUE if in_window else MUTED
        c3 = Circle(tx3, py, r)
        c3.fillColor = col; c3.strokeColor = col
        d.add(c3)

    # VWAP line (approximate)
    d.add(Line(win_start, ty+5, win_end, ty+8, strokeColor=YELLOW, strokeWidth=2))
    dtext(d, win_end - 30, ty+16, "VWAP", sz=7, color=YELLOW, font="Helvetica-Bold")

    dtext(d, tx0 + tw*0.12, ty - 35, "outside window", sz=6, color=MUTED, anchor="middle")
    dtext(d, tx0, 12, "Circle size = trade volume. VWAP = Sum(price x qty) / Sum(qty) over the window.",
          sz=7, color=MUTED, maxch=85)
    return d

def fig_micro_price():
    """Micro-price / depth imbalance visual."""
    dh = 160
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    mid = AVAIL / 2
    bid_p = 50300; ask_p = 50350; bid_q = 3.2; ask_q = 0.8
    total_q = bid_q + ask_q

    # bid bar (left)
    bid_w = (bid_q / total_q) * (mid - 30) * 1.6
    drect(d, mid - 20 - bid_w, 80, bid_w, 30, fill=GREEN_DK, stroke=GREEN, sw=1)
    dtext(d, mid - 20 - bid_w/2, 91, f"BID  {bid_q} BTC", sz=7, color=GREEN, anchor="middle")

    # ask bar (right)
    ask_w = (ask_q / total_q) * (mid - 30) * 1.6
    drect(d, mid + 20, 80, ask_w, 30, fill=RUST_DK, stroke=RUST, sw=1)
    dtext(d, mid + 20 + ask_w/2, 91, f"ASK  {ask_q} BTC", sz=7, color=RUST, anchor="middle")

    # mid-price dot
    simple_mid = (bid_p + ask_p) / 2
    micro = (ask_p * bid_q + bid_p * ask_q) / total_q
    dtext(d, mid, 125, f"Simple mid = {simple_mid:.0f}", sz=8, color=MUTED, anchor="middle")
    dtext(d, mid, 110, f"Micro-price = {micro:.1f}  (pulled toward bigger side)", sz=8, color=YELLOW, anchor="middle")

    imb = (bid_q - ask_q) / total_q
    dtext(d, mid, 55, f"Depth imbalance = {imb:.2f}  (+1 = all bids, -1 = all asks)",
          sz=8, color=PURPLE, anchor="middle")
    dtext(d, mid, 40, f"Here: {imb:.2f} -> strong buying pressure -> price likely moves UP",
          sz=7, color=MUTED, anchor="middle")

    dtext(d, AVAIL/2, 12,
          "Micro-price is the liquidity-weighted center. It predicts short-term direction better than the simple midpoint.",
          sz=7, color=MUTED, anchor="middle", maxch=90)
    return d

def fig_api_stack():
    """Middleware stack for the HTTP API."""
    dh = 200
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    layers = [
        ("CLIENT REQUEST",             MUTED,   PANEL),
        ("TcpListener  (Tokio)",        BLUE,    BLUE_DK),
        ("Axum Router",                 GREEN,   GREEN_DK),
        ("TimeoutLayer  (10s)",         YELLOW,  YEL_DK),
        ("Handler  (read RwLock)",      GREEN,   GREEN_DK),
        ("JSON Serialisation",          PURPLE,  PUR_DK),
        ("HTTP Response",               MUTED,   PANEL),
    ]
    lh = 22; lw = AVAIL - 80; lx = 40
    total_h = len(layers) * lh
    base_y = (dh - total_h) / 2

    for i, (label, col, fill) in enumerate(layers):
        ly2 = base_y + i * lh
        drect(d, lx, ly2, lw, lh-2, fill=fill, stroke=col, sw=1)
        dtext(d, lx + lw/2, ly2 + 7, label[:40], sz=8, color=col, anchor="middle")
        if i < len(layers)-1:
            varrow(d, lx+lw/2, ly2, ly2+lh-2, color=MUTED)

    dtext(d, AVAIL/2, 10,
          "Requests flow downward through each layer. The TimeoutLayer kills slow requests before they block the thread.",
          sz=7, color=MUTED, anchor="middle", maxch=90)
    return d

def fig_docker_layers():
    """Docker multi-stage build layers."""
    dh = 200
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    half = AVAIL / 2 - 10
    # Stage 1
    drect(d, 10, 20, half-10, 170, fill=PANEL, stroke=RUST, sw=1.5)
    dtext(d, 10 + (half-10)/2, 180, "STAGE 1: Builder", font="Helvetica-Bold",
          sz=8, color=RUST, anchor="middle")
    dtext(d, 10 + (half-10)/2, 168, "rust:1.78-slim", sz=7, color=MUTED, anchor="middle")
    stage1 = ["Cargo.toml + Cargo.lock", "dummy src/main.rs", "(deps compiled - cached)",
              "real src/", "cargo build --release", "=> binary 5MB"]
    for i, l in enumerate(stage1):
        col = YELLOW if "cached" in l else (GREEN if "binary" in l else WHITE)
        dtext(d, 20, 152 - i*18, l[:28], sz=7, color=col, font="Courier" if i<2 or i==5 else "Helvetica")

    # Stage 2
    x2 = half + 10
    drect(d, x2, 20, half - 10, 170, fill=PANEL, stroke=GREEN, sw=1.5)
    dtext(d, x2 + (half-10)/2, 180, "STAGE 2: Runtime", font="Helvetica-Bold",
          sz=8, color=GREEN, anchor="middle")
    dtext(d, x2 + (half-10)/2, 168, "distroless/cc-debian12", sz=7, color=MUTED, anchor="middle")
    stage2 = ["COPY binary from stage 1", "ENV SYMBOL=BTCUSDT", "ENV API_PORT=3000",
              "EXPOSE 3000", "CMD [./crypto-orderbook]", "=> image ~10MB, no shell"]
    for i, l in enumerate(stage2):
        col = GREEN if "binary" in l or "image" in l else WHITE
        dtext(d, x2+10, 152 - i*18, l[:28], sz=7, color=col, font="Courier" if i>0 else "Helvetica")

    # arrow between stages
    harrow(d, half-10+10, x2, 110, color=YELLOW, label="COPY binary only")

    dtext(d, AVAIL/2, 8,
          "The compiler never ships. The runtime image has no shell, no package manager, no attack surface.",
          sz=7, color=MUTED, anchor="middle", maxch=85)
    return d

def fig_graceful_shutdown():
    """Graceful shutdown signal propagation."""
    dh = 150
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    # signal
    drect(d, 10, 95, 90, 36, fill=RUST_DK, stroke=RUST, sw=1.5)
    dtext(d, 55, 113, "SIGTERM", font="Helvetica-Bold", sz=8, color=RUST, anchor="middle")

    harrow(d, 100, 150, 113, color=RUST)

    drect(d, 150, 95, 110, 36, fill=PANEL, stroke=BLUE, sw=1)
    dtext(d, 205, 113, "tokio::signal", sz=8, color=BLUE, anchor="middle")

    harrow(d, 260, 310, 113, color=BLUE)

    drect(d, 310, 95, 100, 36, fill=PANEL, stroke=GREEN, sw=1)
    dtext(d, 360, 113, "axum::serve", sz=8, color=GREEN, anchor="middle")
    dtext(d, 360, 101, "stops accepting", sz=6, color=MUTED, anchor="middle")

    # in-flight
    drect(d, 200, 50, 180, 32, fill=YEL_DK, stroke=YELLOW, sw=1)
    dtext(d, 290, 66, "In-flight requests", font="Helvetica-Bold", sz=7, color=YELLOW, anchor="middle")
    dtext(d, 290, 54, "finish then process exits", sz=7, color=MUTED, anchor="middle")
    varrow(d, 310, 82, 95, color=YELLOW)

    dtext(d, AVAIL/2, 12,
          "tokio::select! races the HTTP server future against ctrl_c(). The winner determines shutdown.",
          sz=7, color=MUTED, anchor="middle", maxch=80)
    return d

def fig_production_roadmap():
    """Production readiness checklist."""
    dh = 280
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    items = [
        (GREEN,  "Prometheus /metrics",    "Counters + gauges live on every deployment"),
        (GREEN,  "VWAP Engine",            "1-minute rolling trade-weighted average price"),
        (GREEN,  "Micro-Price",            "Liquidity-weighted midpoint - smarter than (bid+ask)/2"),
        (GREEN,  "Depth Imbalance",        "Buy vs sell pressure signal used by HFT desks"),
        (GREEN,  "Graceful Shutdown",      "SIGTERM drains in-flight before exit"),
        (YELLOW, "WebSocket Push API",     "Push book diffs to subscribers over WS instead of polling"),
        (YELLOW, "Circuit Breaker",        "Stop hammering Binance REST if it is down"),
        (YELLOW, "Rate Limiter",           "Reject burst traffic before it hits the book lock"),
        (RUST,   "TimescaleDB Writes",     "Persist every top-of-book snapshot for backtesting"),
        (RUST,   "Multi-Symbol",           "Fan out to N books, one manager per symbol"),
        (RUST,   "Risk Engine Hook",       "Position limits + P&L calculation on every update"),
        (MUTED,  "Docker Compose",         "Spin up engine + Prometheus + Grafana in one command"),
    ]
    row_h = 21
    col1 = 14; col2 = 165; col3 = 290
    header_y = dh - 22
    dtext(d, col1, header_y, "Status", font="Helvetica-Bold", sz=7, color=WHITE)
    dtext(d, col2, header_y, "Feature", font="Helvetica-Bold", sz=7, color=WHITE)
    dtext(d, col3, header_y, "Why It Matters", font="Helvetica-Bold", sz=7, color=WHITE)

    for i, (col, name, why) in enumerate(items):
        y = header_y - (i+1)*row_h
        if y < 10: break
        status = "BUILT" if col == GREEN else ("NEXT" if col == YELLOW else "PLANNED")
        drect(d, col1-2, y-4, 55, 16, fill=col if col==GREEN else PANEL, stroke=col, sw=0.8)
        dtext(d, col1+25, y+3, status, sz=6, color=BG if col==GREEN else col, anchor="middle",
              font="Helvetica-Bold")
        dtext(d, col2, y+3, name[:22], sz=7, color=col if col==GREEN else WHITE)
        dtext(d, col3, y+3, why[:int((AVAIL-col3-10)//4.5)], sz=6, color=MUTED)
    return d

def fig_error_tree():
    """Error handling decision tree."""
    dh = 190
    d = Drawing(AVAIL, dh)
    drect(d, 0, 0, AVAIL, dh, fill=PANEL)

    # root
    root_x = AVAIL/2 - 60; root_y = 148; bw_e = 120; bh_e = 28
    drect(d, root_x, root_y, bw_e, bh_e, fill=RUST_DK, stroke=RUST, sw=1.5)
    dtext(d, root_x+bw_e/2, root_y+10, "Error Occurs", font="Helvetica-Bold",
          sz=8, color=RUST, anchor="middle")

    branches = [
        (60,  100, "WebSocket\nError",    BLUE,   "reconnect\n+ resync"),
        (220, 100, "Parse\nError",        YELLOW, "log + skip\nframe"),
        (380, 100, "Seq Gap\nor REST fail",RUST,  "clear book\n+ resync"),
    ]
    for bx4, by4, title, col, action in branches:
        # line from root
        d.add(Line(AVAIL/2, root_y, bx4+60, by4+40,
                   strokeColor=col, strokeWidth=1))
        drect(d, bx4, by4, 120, 40, fill=PANEL, stroke=col, sw=1)
        for j, tl in enumerate(title.split("\n")):
            dtext(d, bx4+60, by4+28-j*13, tl[:15], sz=7, color=col, anchor="middle",
                  font="Helvetica-Bold")

        # action box below
        drect(d, bx4+10, by4-46, 100, 40, fill=PANEL, stroke=MUTED, sw=0.8)
        varrow(d, bx4+60, by4, by4-6, color=col)
        for j, al in enumerate(action.split("\n")):
            dtext(d, bx4+60, by4-14-j*13, al[:15], sz=7, color=MUTED, anchor="middle")

    dtext(d, AVAIL/2, 10,
          "Every error has a recovery path. We never panic in production code. AppError variants encode the recovery strategy.",
          sz=7, color=MUTED, anchor="middle", maxch=90)
    return d

# ══════════════════════════════════════════════════════════════════════════════
#  DOCUMENT BUILDER
# ══════════════════════════════════════════════════════════════════════════════

def build():
    doc = SimpleDocTemplate(
        OUT_PATH,
        pagesize=A4,
        leftMargin=MARGIN, rightMargin=MARGIN,
        topMargin=16*mm, bottomMargin=16*mm,
    )

    cw4 = [38*mm, 30*mm, 30*mm, AVAIL - 38*mm - 30*mm - 30*mm - 6]

    story = []

    # ── Cover ──────────────────────────────────────────────────────────────────
    story += [
        SP(40),
        P("Crypto Order Book Engine", "title"),
        P("A production-grade Rust system: design decisions, real-world analogies, and the math behind it.", "sub"),
        HR(),
        P("This document is built alongside the code. Every decision has a reason. "
          "Every reason is grounded in how real markets actually work.", "muted"),
        PageBreak(),
    ]

    # ── PART 0: The Trading Floor ──────────────────────────────────────────────
    story += [
        P("Part 0 - What Is an Order Book?", "h1"),
        HR(),
        P("Imagine a farmers market where every vendor has a chalkboard showing what they will sell "
          "and at what price. Buyers walk around with their own boards showing what they want to buy. "
          "When a buyer's price matches a seller's price, a trade happens and both boards are erased. "
          "A stock or crypto exchange is exactly this - except it runs in RAM, processes millions of "
          "updates per second, and the boards change every millisecond.", "body"),
        P("The order book is the chalkboard. Our engine is the person watching that chalkboard "
          "continuously so that when you ask 'what is Bitcoin worth right now?', we answer in under "
          "a millisecond without calling Binance.", "body"),
        SP(4),
        fig_trading_floor(),
        CAP("Figure 1 - Buyers, the exchange floor, and sellers. Our system mirrors this structure in code."),
        SP(8),
        P("Why does this matter?", "h3"),
        P("The gap between the best buy price and the best sell price is called the spread. "
          "It is the cost of doing business urgently. A tight spread means a liquid market - "
          "you can buy and sell quickly without moving the price. A wide spread means illiquidity. "
          "Every trading system in the world cares deeply about this number.", "body"),
        P("What we built is an engine that maintains a perfect real-time copy of Binance's order book "
          "in local memory. It connects over WebSocket, handles network failures automatically, "
          "validates every incoming message for correctness, and exposes a fast HTTP API "
          "so any service can query the current state of the market.", "body"),
        PageBreak(),
    ]

    # ── PART 1: Architecture ───────────────────────────────────────────────────
    story += [
        P("Part 1 - System Architecture", "h1"),
        HR(),
        P("The system has four actors. Each runs independently in its own Tokio async task. "
          "They communicate only through typed channels and shared memory references. "
          "This is the actor model - a design pattern used by every major trading system.", "body"),
        SP(4),
        fig_actors(),
        CAP("Figure 2 - The four actors. Arrows show data flow. Arc<RwLock> means shared read access."),
        SP(8),
        P("Why separate actors?", "h3"),
        P("If the WebSocket drops, only the WS Client task restarts. The order book keeps "
          "its data. The HTTP API keeps serving cached data. Each part can fail and recover "
          "without taking down the others.", "body"),
        P("The channel between the WS Client and Manager is a buffered queue with 10,000 slots. "
          "When the Manager goes offline to fetch a REST snapshot (which takes ~100ms), "
          "the WS Client keeps receiving messages and storing them in the buffer. "
          "Nothing is lost. This is the key insight of the whole design.", "body"),
        P("The Arc<RwLock<OrderBook>> shared between the Manager and the API is how we achieve "
          "concurrent reads with exclusive writes. Think of it as a shared whiteboard in a newsroom: "
          "anyone can read it at the same time, but only one person can write on it at a time, "
          "and they have to wait until all readers step back.", "body"),
    ]

    story += [
        P("Part 2 - Why Rust?", "h1"),
        HR(),
        P("Most financial systems are written in C++ or Java. C++ is fast but has no memory safety. "
          "Java is safe but has a garbage collector that can pause your program at any moment - "
          "unacceptable when a 50ms pause means you acted on stale data.", "body"),
        P("Rust gives you both: memory safety enforced at compile time, and zero pauses at runtime. "
          "The compiler rejects programs with data races before they ever run. "
          "This is not a hypothetical benefit - Rust is now used by Cloudflare, Discord, "
          "and several high-frequency trading firms precisely because of this.", "body"),
        ptable(
            ["Decision", "We Chose", "Alternative", "Why We Chose It"],
            [
                ["Price type",      "rust_decimal",  "f64 float",    "f64 has silent rounding errors. 0.1 + 0.2 != 0.3 in floating point. Financial systems cannot have invisible rounding in price calculations."],
                ["Async runtime",   "tokio",         "std threads",  "One OS thread per WebSocket connection does not scale. Tokio multiplexes thousands of async tasks onto a handful of threads with near-zero overhead."],
                ["HTTP framework",  "axum",          "actix-web",    "actix-web uses its own actor runtime that conflicts with Tokio. axum is built on top of Tokio natively - no impedance mismatch."],
                ["Error handling",  "thiserror",     "anyhow",       "anyhow erases error types into a single blob. We need to inspect each variant to decide: reconnect, skip, or resync. thiserror preserves that structure."],
                ["Logging",         "tracing",       "println!",     "println! has no concept of which async task is running. tracing attaches structured context (task ID, symbol, span) to every log line automatically."],
                ["WebSocket",       "tokio-tungstenite","raw TCP",   "WebSocket framing, masking, and ping/pong handling is hundreds of lines of protocol code. tokio-tungstenite does all of it, tested and correct."],
            ],
            cw4,
        ),
        SP(6),
        PageBreak(),
    ]

    # ── PART 3: Error taxonomy ─────────────────────────────────────────────────
    story += [
        P("Part 3 - Error Taxonomy", "h1"),
        HR(),
        P("Most systems use one error type for everything and then write giant if-else chains to handle them. "
          "We use a typed enum where each variant encodes its own recovery strategy. "
          "The compiler forces you to handle every case.", "body"),
        P("This is not academic tidiness. It is engineering discipline. When a SequenceGap arrives at 3am "
          "and wakes someone up, the error message tells them exactly what happened and why.", "body"),
        SP(4),
        fig_error_tree(),
        CAP("Figure 3 - Every error has a path. No error is fatal unless it reaches main()."),
        SP(8),
        P("The error variants:", "h3"),
        P("WebSocket(tungstenite::Error) - The network dropped. Recovery: reconnect with backoff, send Reconnected signal, trigger resync.", "body"),
        P("Parse(serde_json::Error) - Binance sent a malformed JSON frame. Recovery: log it, skip that frame, keep going. One bad frame does not invalidate the whole stream.", "body"),
        P("InvalidField(String) - A price or quantity field is not a valid decimal. Same recovery as Parse.", "body"),
        P("SequenceGap { expected, got } - We received sequence 112 when we expected 111. The missed event cannot be recovered from the stream. Recovery: clear the book, restart the sync cycle from the snapshot.", "body"),
        P("SyncFailed - The first event we received after a snapshot does not connect cleanly to that snapshot. Recovery: same as SequenceGap - full resync.", "body"),
        PageBreak(),
    ]

    # ── PART 4: WebSocket Client ───────────────────────────────────────────────
    story += [
        P("Part 4 - The WebSocket Client", "h1"),
        HR(),
        P("A WebSocket is like a phone call you make to Binance and then leave open permanently. "
          "Binance reads market updates into the phone continuously. Your job is to listen and never hang up. "
          "If the call drops, you call back immediately. If it keeps dropping, you wait a bit longer each time.", "body"),
        SP(4),
        fig_ws_states(),
        CAP("Figure 4 - The WS client state machine. It stays alive as long as the process runs."),
        SP(8),
        P("Exponential backoff with jitter", "h3"),
        P("When a connection fails, we wait before retrying. The wait doubles each time: 1s, 2s, 4s, 8s, up to 30s. "
          "This is exponential backoff. The reason: if Binance's servers are restarting, "
          "a thousand clients all reconnecting simultaneously would create a thundering herd - "
          "a flood that prevents the server from recovering.", "body"),
        P("Jitter adds a random amount (up to 1 second) to each wait. This spreads the reconnect attempts "
          "across time so no two clients retry at exactly the same moment.", "body"),
        SP(4),
        fig_backoff(),
        CAP("Figure 5 - Backoff curve. Capped at 30s. Blue bars show the jitter range per attempt."),
        SP(8),
        P("The Reconnected signal", "h3"),
        P("When we reconnect, the first thing we send into the channel is WsEvent::Reconnected - "
          "before any messages. The Manager sees this and knows: 'everything I buffered from the old "
          "connection is stale. Throw it away and start a fresh sync cycle.'", "body"),
        P("This is crucial because after a reconnect, the first messages in the buffer might have "
          "sequence numbers that pre-date the disconnect. Applying them would silently corrupt the book.", "body"),
        PageBreak(),
    ]

    # ── PART 5: Binance Protocol ───────────────────────────────────────────────
    story += [
        P("Part 5 - The Binance Sync Protocol", "h1"),
        HR(),
        P("You cannot just connect to Binance's depth stream and start applying updates. "
          "The stream is a continuous diff feed - it tells you what changed, not what the current state is. "
          "If you join mid-stream, you have no baseline to apply the diffs to.", "body"),
        P("Think of it like joining a running commentary of a chess game. The commentator says "
          "'white moved pawn from e2 to e4'. If you did not see the board before that move, "
          "you cannot reconstruct the current position.", "body"),
        SP(4),
        fig_binance_protocol(),
        CAP("Figure 6 - The sync timeline. Events buffer in our channel while we fetch the REST snapshot."),
        SP(8),
        P("The handshake algorithm:", "h3"),
        P("Step 1: Open the WebSocket and start receiving events. They buffer in the mpsc channel.", "body"),
        P("Step 2: Separately, call Binance's REST API to get a full snapshot. This snapshot has a lastUpdateId.", "body"),
        P("Step 3: Apply the snapshot to the book. This is our baseline.", "body"),
        P("Step 4: Drain the buffered events. Discard any event where last_update_id <= snapshot_id (already included in snapshot).", "body"),
        P("Step 5: The first event where first_update_id <= snapshot_id + 1 AND last_update_id > snapshot_id is our connection point. Apply it.", "body"),
        P("Step 6: From here, each event's first_update_id must equal the previous event's last_update_id + 1. A gap means we lost data. Resync.", "body"),
        SP(4),
        fig_sequence_check(),
        CAP("Figure 7 - Sequence number continuity check. A gap triggers a full resync."),
        SP(8),
        ptable(
            ["Scenario", "Detection", "Recovery"],
            [
                ["Normal live update",          "U == prev_u + 1",                "Apply to book, update prev_u"],
                ["Stale event (in snapshot)",   "u <= snapshot_id",               "Discard silently"],
                ["Gap in handshake",            "U > snapshot_id + 1",            "Clear book, restart sync cycle"],
                ["Gap in live mode",            "U != prev_u + 1",                "Clear book, restart sync cycle"],
                ["WebSocket drops",             "WsEvent::Reconnected received",  "Clear book, restart sync cycle"],
                ["REST snapshot fails",         "HTTP error or bad JSON",          "Retry with exponential backoff"],
            ],
            [55*mm, 65*mm, AVAIL - 55*mm - 65*mm - 6],
        ),
        PageBreak(),
    ]

    # ── PART 6: The Parser ─────────────────────────────────────────────────────
    story += [
        P("Part 6 - Two-Layer Parsing", "h1"),
        HR(),
        P("Binance's JSON format has field names like 'U', 'u', 'b', 'a'. Those are Binance's internal names. "
          "If we used those names throughout our codebase, every module would know about Binance's naming convention. "
          "If Binance ever changes their API, we would have to change code everywhere.", "body"),
        SP(4),
        fig_two_layer(),
        CAP("Figure 8 - Wire format is private to binance.rs. Domain type is what the rest of the system uses."),
        SP(8),
        P("The two-layer pattern:", "h3"),
        P("Layer 1 (private): RawDepthUpdate matches Binance's exact JSON. Field names are 'U', 'u', 'b', 'a'. "
          "This struct lives only in binance.rs.", "body"),
        P("Layer 2 (public): DepthUpdate uses readable names: first_update_id, last_update_id, bids, asks. "
          "All types are validated Decimal values, not raw strings.", "body"),
        P("The parse() function in between is the only place that knows Binance's format. "
          "It validates every field and returns an AppError if anything is malformed. "
          "The rest of the system works with clean domain types.", "body"),
        PageBreak(),
    ]

    # ── PART 7: Order Book ─────────────────────────────────────────────────────
    story += [
        P("Part 7 - The Order Book Data Structure", "h1"),
        HR(),
        P("We store bids and asks in a BTreeMap<Decimal, Decimal>: price -> quantity. "
          "BTreeMap keeps its keys sorted at all times, always. Insert a new price, it lands in the right place. "
          "Remove a price, the others shift to fill the gap. This is not free - each operation is O(log n) - "
          "but for an order book it is exactly what we need.", "body"),
        SP(4),
        fig_btree(),
        CAP("Figure 9 - BTreeMap layout for bids and asks. Best bid = last entry. Best ask = first entry."),
        SP(8),
        P("Why not a HashMap?", "h3"),
        P("A HashMap is O(1) for lookup but unordered. To find the best bid with a HashMap, "
          "you would have to scan every entry. A BTreeMap's last() is O(log n) for the maximum key. "
          "For a book with 5000 price levels, log2(5000) is about 12 operations. Fast enough.", "body"),
        P("The zero-quantity deletion signal:", "h3"),
        P("Binance does not send a 'DELETE price level X' message. Instead, it sends a normal update "
          "for that price with a quantity of zero. Our is_removal() method checks for this. "
          "If we missed this signal, deleted price levels would stay in our book forever as ghost entries.", "body"),
        SP(4),
        fig_zero_qty(),
        CAP("Figure 10 - Before and after a zero-quantity update. Price 50,299 is removed from the book."),
        PageBreak(),
    ]

    # ── PART 8: Market Microstructure ─────────────────────────────────────────
    story += [
        P("Part 8 - Market Microstructure Signals", "h1"),
        HR(),
        P("An order book is not just a list of prices. It is a map of market psychology. "
          "Every level, every quantity, every update tells you something about what buyers and sellers "
          "believe Bitcoin is worth and how urgently they want to trade.", "body"),
        P("Professional trading desks compute signals from the book continuously. "
          "We have implemented three of the most important ones.", "body"),
        SP(4),
        fig_micro_price(),
        CAP("Figure 11 - Micro-price pulls toward the side with more liquidity. Depth imbalance quantifies the pressure."),
        SP(8),
        P("Spread - the cost of urgency", "h3"),
        P("The spread is the difference between the best ask and the best bid. "
          "If you want to buy Bitcoin right now, you pay the ask. If you want to sell right now, "
          "you get the bid. The spread is what you lose by acting immediately instead of waiting. "
          "Market makers earn the spread. Takers pay it.", "body"),
        P("Micro-price (weighted mid-price)", "h3"),
        P("The arithmetic midpoint (bid + ask) / 2 treats both sides equally. "
          "But if there are 3 BTC waiting to buy and only 0.5 BTC waiting to sell, "
          "the market is more likely to move up (buyers outnumber sellers). "
          "Micro-price weights the midpoint by the opposite side's quantity. "
          "It predicts short-term price direction better than the simple midpoint.", "body"),
        P("The formula: micro_price = (ask_price * bid_qty + bid_price * ask_qty) / (bid_qty + ask_qty)", "code"),
        P("Depth Imbalance", "h3"),
        P("Imbalance = (bid_qty - ask_qty) / (bid_qty + ask_qty). "
          "Range: -1 (all sellers, price likely falls) to +1 (all buyers, price likely rises). "
          "Values above 0.3 or below -0.3 are considered strong signals by most market microstructure models.", "body"),
        SP(4),
        fig_vwap(),
        CAP("Figure 12 - Rolling VWAP window. Each circle is a trade. Size reflects volume. Grey = outside window."),
        SP(8),
        P("VWAP - Volume Weighted Average Price", "h3"),
        P("Every institutional fund benchmarks their execution against VWAP. "
          "If you bought Bitcoin at a price above the day's VWAP, you paid more than average. "
          "If you bought below, you were a good executor. "
          "We compute a 1-minute rolling VWAP from Binance's trade stream - a separate WebSocket "
          "subscription that delivers every individual trade in real time.", "body"),
        P("VWAP formula: Sum(price_i * quantity_i) / Sum(quantity_i) over the window", "code"),
        P("We implement this with a VecDeque ring buffer that evicts old trades as the window slides forward. "
          "The sum of price*quantity and sum of quantity are maintained incrementally so each new trade "
          "is O(1) to add and each evicted trade is O(1) to remove.", "body"),
        PageBreak(),
    ]

    # ── PART 9: Manager ───────────────────────────────────────────────────────
    story += [
        P("Part 9 - The Manager State Machine", "h1"),
        HR(),
        P("The Manager is the most complex part of the system. It is a state machine with three phases, "
          "and any error in any phase sends it back to the start. "
          "Think of a newspaper archivist who must always have the complete, up-to-date archive. "
          "If a batch of papers is lost, they do not try to reconstruct it. "
          "They get the current edition and start fresh.", "body"),
        SP(4),
        fig_manager_fsm(),
        CAP("Figure 13 - The Manager state machine. Any error in Phase 2 or 3 restarts from Phase 1."),
        SP(8),
        P("Phase 1: Snapshot Fetch", "h3"),
        P("Call Binance's REST API: GET /api/v3/depth?symbol=BTCUSDT&limit=5000. "
          "This returns up to 5000 price levels on each side and a lastUpdateId. "
          "If the request fails, we retry with exponential backoff. We never give up. "
          "The WebSocket client is still running and buffering events throughout.", "body"),
        P("Phase 2: Handshake", "h3"),
        P("Drain the buffered events until we find the first one that connects to the snapshot's lastUpdateId. "
          "Apply that event. Return its lastUpdateId as the starting point for Phase 3. "
          "If we receive a Reconnected signal during this phase, the buffer is poisoned - restart.", "body"),
        P("Phase 3: Live Mode", "h3"),
        P("Process events indefinitely. For each event, check that its first_update_id equals "
          "the previous event's last_update_id + 1. If yes, apply and advance. "
          "If no, we missed at least one event. Clear the book and go back to Phase 1.", "body"),
        P("Why std::sync::RwLock instead of tokio::sync::RwLock?", "h3"),
        P("Because we never await inside the lock. The book operations (apply, read) are pure "
          "in-memory computation with no I/O. std::sync::RwLock is faster for synchronous "
          "critical sections because it does not need to interact with the async scheduler. "
          "This is one of the subtler Rust async decisions.", "body"),
        P("The Arc wraps the RwLock because both the Manager (writer) and the API handlers (readers) "
          "need to hold a reference to the same book across async task boundaries. "
          "Arc provides shared ownership with reference counting.", "body"),
        SP(4),
        fig_arc_rwlock(),
        CAP("Figure 14 - Arc<RwLock<>> allows one writer and many concurrent readers to share the book safely."),
        PageBreak(),
    ]

    # ── PART 10: Metrics ──────────────────────────────────────────────────────
    story += [
        P("Part 10 - Metrics and Observability", "h1"),
        HR(),
        P("A system you cannot observe is a system you cannot operate. "
          "In production, you will not be watching logs. You will be watching dashboards. "
          "When something goes wrong at 3am, you need to know whether it happened because of "
          "a network issue (reconnect counter spiking), a Binance problem (snapshot fetch errors), "
          "or a logic bug (sequence gap counter non-zero).", "body"),
        P("We expose all metrics in Prometheus format at GET /metrics. Prometheus scrapes it every 15 seconds. "
          "Grafana graphs it. PagerDuty alerts when thresholds breach.", "body"),
        SP(4),
        fig_metrics_overview(),
        CAP("Figure 15 - The three Prometheus metric types and what we use each for."),
        SP(8),
        ptable(
            ["Metric Name", "Type", "What a spike means"],
            [
                ["orderbook_ws_reconnects_total",    "Counter", "Network instability or Binance outage. Should be near-zero in healthy operation."],
                ["orderbook_sequence_gaps_total",    "Counter", "We are missing updates. Data integrity risk. Alert on any non-zero value."],
                ["orderbook_snapshots_total",        "Counter", "Every gap triggers a new snapshot fetch. Correlated with sequence_gaps."],
                ["orderbook_updates_applied_total",  "Counter", "Throughput. Should grow steadily. Plateau means the stream stopped."],
                ["orderbook_bid_depth",              "Gauge",   "If this drops to zero and stays there, the book is unsynced. Check health endpoint."],
                ["orderbook_spread",                 "Gauge",   "Sudden spike = unusual market conditions. Worth alerting on if it persists."],
                ["orderbook_vwap_1m",                "Gauge",   "Current 1-minute VWAP. Compare to best_bid/best_ask to understand market direction."],
            ],
            [68*mm, 24*mm, AVAIL - 68*mm - 24*mm - 6],
        ),
        PageBreak(),
    ]

    # ── PART 11: HTTP API ─────────────────────────────────────────────────────
    story += [
        P("Part 11 - The HTTP API", "h1"),
        HR(),
        P("The API is intentionally simple: read the book, serialize to JSON, return. "
          "No business logic. No writes. The only state it touches is the read side of the RwLock.", "body"),
        SP(4),
        fig_api_stack(),
        CAP("Figure 16 - HTTP request flow through the middleware stack."),
        SP(8),
        ptable(
            ["Endpoint", "Returns", "Use case"],
            [
                ["GET /health",            "200 if synced, 503 if syncing",                   "Load balancer health check. Do not route traffic until this returns 200."],
                ["GET /book/best",         "best_bid, best_ask, spread, last_update_id",       "The single most queried endpoint. What is the price right now?"],
                ["GET /book/snapshot",     "top N bids and asks (default 20, max 100)",        "Market depth display. Shows where volume is sitting."],
                ["GET /book/midprice",     "Micro-price (liquidity weighted)",                 "Better than (bid+ask)/2 for estimating true fair value."],
                ["GET /book/imbalance",    "Imbalance ratio and interpretation string",        "Buy vs sell pressure signal. Range -1 to +1."],
                ["GET /book/vwap",         "1-minute rolling VWAP from trade stream",          "Execution benchmark. Compare your fill price against this."],
                ["GET /metrics",           "Prometheus text format",                           "Scraped by Prometheus. Graphed in Grafana. Drives alerts."],
            ],
            [48*mm, 75*mm, AVAIL - 48*mm - 75*mm - 6],
        ),
        SP(8),
        P("The AppState pattern:", "h3"),
        P("Axum clones AppState into every request handler. Because AppState only holds an Arc "
          "(reference-counted pointer), the clone is O(1) - it just increments a counter. "
          "The actual order book is never copied. This is how we serve thousands of concurrent "
          "requests with a single in-memory book.", "body"),
        P("Timeout middleware:", "h3"),
        P("Every request is wrapped in a 10-second timeout from tower-http's TimeoutLayer. "
          "If a handler stalls (for example, the RwLock is contended), "
          "the middleware kills the request and returns 408. "
          "This prevents a slow query from holding threads forever.", "body"),
        PageBreak(),
    ]

    # ── PART 12: Config & Wiring ──────────────────────────────────────────────
    story += [
        P("Part 12 - Configuration and Wiring", "h1"),
        HR(),
        P("All configuration enters through environment variables, read once at startup. "
          "The Config struct is the single source of truth for every tunable parameter. "
          "This is the twelve-factor app principle: configuration in the environment.", "body"),
        ptable(
            ["Env Var", "Default", "What It Controls"],
            [
                ["SYMBOL",         "BTCUSDT",                                      "Which trading pair to track. Change to ETHUSDT to track Ethereum."],
                ["API_PORT",       "3000",                                          "Which port to bind the HTTP server on."],
                ["WS_BASE",        "wss://stream.binance.com:9443/ws/{symbol}@depth","WebSocket URL. Override for testnet or a different exchange."],
                ["TRADE_WS_BASE",  "wss://stream.binance.com:9443/ws/{symbol}@trade","Trade stream URL for VWAP calculation."],
                ["REST_BASE",      "https://api.binance.com",                       "REST API base URL. Override for Binance testnet."],
                ["CHANNEL_BUFFER", "10000",                                         "How many WS events to buffer during snapshot fetch. 10k = ~10 seconds of Binance updates."],
                ["RUST_LOG",       "info",                                          "Log verbosity. Set to debug for every event. info for normal ops."],
            ],
            [38*mm, 38*mm, AVAIL - 38*mm - 38*mm - 6],
        ),
        SP(8),
        P("How main.rs wires everything together:", "h3"),
        P("main() is intentionally short. It reads config, builds shared state, spawns background tasks, "
          "and then drives the HTTP server in the foreground. When main() returns, the process exits. "
          "All background tasks are killed automatically because Tokio ties task lifetimes to the runtime.", "body"),
        P("1. Config::from_env() - read all configuration", "body"),
        P("2. Arc::new(RwLock::new(OrderBook::new())) - create the shared book", "body"),
        P("3. mpsc::channel(buffer) - create the event pipeline", "body"),
        P("4. ws::client::spawn(url, tx) - start depth WebSocket in background", "body"),
        P("5. ws::client::spawn(trade_url, trade_tx) - start trade WebSocket in background", "body"),
        P("6. tokio::spawn(Manager::new(rx, book, symbol).run()) - start the manager", "body"),
        P("7. tokio::spawn(TradeManager::new(trade_rx, vwap).run()) - start trade VWAP manager", "body"),
        P("8. metrics::init_all() - force-initialize all Prometheus counters to zero", "body"),
        P("9. axum::serve(listener, router(state)).await - serve HTTP until SIGTERM", "body"),
        PageBreak(),
    ]

    # ── PART 13: Docker ───────────────────────────────────────────────────────
    story += [
        P("Part 13 - Docker and Deployment", "h1"),
        HR(),
        P("The Dockerfile has two stages. Stage 1 compiles the binary. Stage 2 runs it. "
          "Only the binary crosses the boundary. The final image contains no compiler, "
          "no Rust toolchain, no shell, no package manager. "
          "It is harder to exploit a system that has no tools to exploit with.", "body"),
        SP(4),
        fig_docker_layers(),
        CAP("Figure 17 - Two-stage Docker build. Only the compiled binary ships to production."),
        SP(8),
        P("Why distroless?", "h3"),
        P("The gcr.io/distroless/cc-debian12 base image contains only the C standard library "
          "and its dependencies - the absolute minimum needed to run a compiled binary. "
          "No bash, no curl, no apt. If an attacker somehow breaks out of the binary, "
          "they land in an empty environment with no tools. This is the principle of least privilege.", "body"),
        P("Dependency layer caching:", "h3"),
        P("We copy Cargo.toml and Cargo.lock before the source code and build a stub binary first. "
          "Docker caches each layer. If you change only src/ (not Cargo.toml), "
          "the dependency compilation layer is reused. "
          "This turns a 3-minute build into a 15-second build.", "body"),
        SP(4),
        fig_graceful_shutdown(),
        CAP("Figure 18 - Graceful shutdown. SIGTERM drains in-flight requests before the process exits."),
        SP(8),
        P("Graceful shutdown prevents two problems:", "h3"),
        P("1. Clients mid-request get a clean response instead of a broken connection.", "body"),
        P("2. If the Manager is mid-write to the book when SIGTERM arrives, "
          "the RwLock ensures the write completes before the process exits. "
          "We never write half an update.", "body"),
        PageBreak(),
    ]

    # ── PART 14: Production Roadmap ───────────────────────────────────────────
    story += [
        P("Part 14 - Production Readiness Roadmap", "h1"),
        HR(),
        P("What we have built is production-capable for a single symbol at moderate load. "
          "A real trading infrastructure system would add the following, roughly in priority order. "
          "Each item is a standalone engineering project.", "body"),
        SP(4),
        fig_production_roadmap(),
        CAP("Figure 19 - Production roadmap. Green = built. Yellow = next. Red = planned."),
        SP(8),
        ptable(
            ["Feature", "Why It Matters in Production"],
            [
                ["WebSocket Push API",
                 "Polling REST adds latency and server load. A push model delivers updates to subscribers "
                 "the moment the book changes. Every professional trading client uses WebSocket subscriptions."],
                ["Circuit Breaker for REST",
                 "If Binance's REST snapshot endpoint is degraded (slow but not failing), "
                 "our retry backoff may not trigger. A circuit breaker opens after N consecutive slow responses "
                 "and stops sending requests for a cooling period."],
                ["Rate Limiter on HTTP API",
                 "Without rate limiting, one runaway client can monopolize the RwLock read capacity. "
                 "Tower's RateLimit layer allows N requests per second per IP before returning 429."],
                ["TimescaleDB Persistence",
                 "Every top-of-book snapshot, every spread measurement, every VWAP datapoint is a "
                 "time-series record. TimescaleDB (PostgreSQL extension) stores billions of rows efficiently. "
                 "This is the foundation for backtesting and strategy research."],
                ["Multi-Symbol Fan-out",
                 "One Manager per symbol, shared thread pool, separate books. "
                 "The current design scales to this with minimal changes: "
                 "Config becomes a Vec<Config>, and main spawns N manager pairs."],
                ["Risk Engine Hook",
                 "Every time the book updates, emit an event to a risk engine that checks "
                 "open positions against current market price. If exposure exceeds limits, "
                 "send a kill-switch signal. This is regulatory requirement in most jurisdictions."],
            ],
            [50*mm, AVAIL - 50*mm - 6],
        ),
        SP(12),
        P("What makes this system interview-ready:", "h3"),
        P("Every choice in this codebase has a reason you can explain clearly. "
          "BTreeMap because we need sorted extremes. mpsc channel with large buffer because "
          "snapshot fetches take 100ms and we cannot drop 100ms worth of updates. "
          "std::sync::RwLock not tokio's because no await inside the lock. "
          "Two-layer parsing to isolate the Binance dependency. "
          "Exponential backoff with jitter to prevent thundering herds.", "body"),
        P("These are the answers senior engineers give. Not 'I used X because it was popular' "
          "but 'I used X because of specific property Y which this problem requires, "
          "and I considered alternative Z but rejected it because of tradeoff W.'", "body"),
        SP(20),
        HR(),
        P("Built with Rust 1.78 + Tokio 1 + Axum 0.7 + Prometheus 0.13", "muted"),
        P("github.com/Mattbusel/crypto-orderbook", "muted"),
    ]

    doc.build(story, onFirstPage=on_page, onLaterPages=on_page)
    print(f"PDF written: {OUT_PATH}")

if __name__ == "__main__":
    build()
