"""
Crypto Order Book Engine - Living Design Document
Rebuilt for legibility: all diagram elements bounded, text wraps, no overflow.
"""

from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import ParagraphStyle
from reportlab.lib.units import mm
pt = 1  # 1 point = 1 unit in reportlab coordinates
from reportlab.lib import colors
from reportlab.platypus import (
    SimpleDocTemplate, Paragraph, Spacer, HRFlowable,
    Table, TableStyle, PageBreak, KeepTogether
)
from reportlab.graphics.shapes import Drawing, Rect, String, Line, Polygon, Circle
from reportlab.lib.enums import TA_CENTER, TA_LEFT
import os

OUT_PATH = os.path.join(os.path.dirname(__file__), "crypto_orderbook_design.pdf")

W, H = A4  # 595 x 842 points
DW = W - 44*mm  # usable drawing width ~470pt

# ── Palette ────────────────────────────────────────────────────────────────────
BG      = colors.HexColor("#0D1117")
DARK    = colors.HexColor("#161B22")
DARKER  = colors.HexColor("#0d1117")
BORDER  = colors.HexColor("#30363D")
RUST    = colors.HexColor("#E05C42")
BLUE    = colors.HexColor("#58A6FF")
GREEN   = colors.HexColor("#3FB950")
YELLOW  = colors.HexColor("#D29922")
PURPLE  = colors.HexColor("#BC8CFF")
WHITE   = colors.HexColor("#E6EDF3")
MUTED   = colors.HexColor("#8B949E")
RED_BG  = colors.HexColor("#2a1010")
GRN_BG  = colors.HexColor("#0f2a1a")
BLU_BG  = colors.HexColor("#0f1a2a")
YEL_BG  = colors.HexColor("#2a2000")

# ── Typography ─────────────────────────────────────────────────────────────────
def S(name, **kw):
    return ParagraphStyle(name, **kw)

ST = {
    "title":    S("title",    fontName="Helvetica-Bold",   fontSize=30, textColor=WHITE,  spaceAfter=6,  leading=36),
    "subtitle": S("sub",      fontName="Helvetica",        fontSize=13, textColor=BLUE,   spaceAfter=18, leading=18),
    "h1":       S("h1",       fontName="Helvetica-Bold",   fontSize=18, textColor=RUST,   spaceBefore=20,spaceAfter=8,  leading=22),
    "h2":       S("h2",       fontName="Helvetica-Bold",   fontSize=13, textColor=BLUE,   spaceBefore=14,spaceAfter=5,  leading=17),
    "h3":       S("h3",       fontName="Helvetica-Bold",   fontSize=11, textColor=YELLOW, spaceBefore=10,spaceAfter=4,  leading=15),
    "body":     S("body",     fontName="Helvetica",        fontSize=10, textColor=WHITE,  spaceAfter=8,  leading=16),
    "body2":    S("body2",    fontName="Helvetica",        fontSize=9,  textColor=MUTED,  spaceAfter=6,  leading=14),
    "code":     S("code",     fontName="Courier",          fontSize=8,  textColor=GREEN,  spaceAfter=4,  leading=12,
                  backColor=DARK, leftIndent=10, rightIndent=10, spaceBefore=4),
    "caption":  S("caption",  fontName="Helvetica-Oblique",fontSize=8,  textColor=MUTED,  spaceAfter=12, leading=11, alignment=TA_CENTER),
    "roadmap":  S("roadmap",  fontName="Helvetica-Bold",   fontSize=10, textColor=PURPLE, spaceAfter=3,  leading=14),
}

def P(text, style="body"):   return Paragraph(text, ST[style])
def HR():                    return HRFlowable(width="100%", thickness=0.5, color=BORDER, spaceAfter=8, spaceBefore=4)
def SP(n=8):                 return Spacer(1, n)
def CAP(text):               return P(text, "caption")

# ── Safe string helper - clips label to fit box ────────────────────────────────
def ds(d, x, y, text, font="Helvetica", size=8, color=WHITE, anchor="start", maxw=None):
    """Draw a string, optionally clipped to maxw points."""
    if maxw:
        chars_fit = int(maxw / (size * 0.55))
        text = text[:chars_fit]
    d.add(String(x, y, text, fontName=font, fontSize=size, fillColor=color, textAnchor=anchor))

# ── Tradeoff table ─────────────────────────────────────────────────────────────
def tradeoff_table(rows):
    header = ["Decision", "We Chose", "Alternative", "Why"]
    data   = [header] + rows
    cw     = [38*mm, 32*mm, 32*mm, 62*mm]
    t = Table(data, colWidths=cw)
    t.setStyle(TableStyle([
        ("BACKGROUND",    (0,0),(-1,0),  RUST),
        ("TEXTCOLOR",     (0,0),(-1,0),  WHITE),
        ("FONTNAME",      (0,0),(-1,0),  "Helvetica-Bold"),
        ("FONTSIZE",      (0,0),(-1,0),  8),
        ("BACKGROUND",    (0,1),(-1,-1), DARK),
        ("TEXTCOLOR",     (0,1),(-1,-1), WHITE),
        ("FONTNAME",      (0,1),(-1,-1), "Helvetica"),
        ("FONTSIZE",      (0,1),(-1,-1), 8),
        ("ROWBACKGROUNDS",(0,1),(-1,-1), [DARK, colors.HexColor("#1C2128")]),
        ("GRID",          (0,0),(-1,-1), 0.4, BORDER),
        ("VALIGN",        (0,0),(-1,-1), "TOP"),
        ("TOPPADDING",    (0,0),(-1,-1), 5),
        ("BOTTOMPADDING", (0,0),(-1,-1), 5),
        ("LEFTPADDING",   (0,0),(-1,-1), 5),
        ("RIGHTPADDING",  (0,0),(-1,-1), 5),
    ]))
    return t

# ── Page background ────────────────────────────────────────────────────────────
def on_page(canvas, doc):
    canvas.saveState()
    canvas.setFillColor(BG)
    canvas.rect(0, 0, W, H, fill=1, stroke=0)
    canvas.setFillColor(DARK)
    canvas.rect(0, 0, W, 16*mm, fill=1, stroke=0)
    canvas.setFillColor(MUTED)
    canvas.setFont("Helvetica", 7)
    canvas.drawString(18*mm, 6*mm, "Crypto Order Book Engine  |  System Design Document")
    canvas.drawRightString(W-18*mm, 6*mm, f"Page {doc.page}")
    canvas.restoreState()


# ==============================================================================
#  DIAGRAMS
# ==============================================================================

def draw_box(d, x, y, w, h, fill, stroke, label, label_color=WHITE, sublabel=None):
    d.add(Rect(x, y, w, h, rx=5, ry=5, fillColor=fill, strokeColor=stroke, strokeWidth=1.5))
    ly = y + h/2 + (4 if sublabel else 0)
    ds(d, x+w/2, ly, label, font="Helvetica-Bold", size=8, color=label_color, anchor="middle")
    if sublabel:
        ds(d, x+w/2, y+h/2-7, sublabel, font="Helvetica", size=7, color=MUTED, anchor="middle")

def arrow_h(d, x1, y, x2, color=MUTED, label=""):
    d.add(Line(x1, y, x2-6, y, strokeColor=color, strokeWidth=1.3))
    d.add(Polygon([x2, y, x2-7, y+3.5, x2-7, y-3.5], fillColor=color, strokeColor=color))
    if label:
        ds(d, (x1+x2)/2, y+4, label, size=6.5, color=color, anchor="middle")

def arrow_v(d, x, y1, y2, color=MUTED):
    d.add(Line(x, y1, x, y2+6, strokeColor=color, strokeWidth=1.3))
    d.add(Polygon([x, y2, x-3.5, y2+7, x+3.5, y2+7], fillColor=color, strokeColor=color))


# ── Fig 1: System overview ─────────────────────────────────────────────────────
def fig_system_overview():
    d = Drawing(DW, 70)
    bw, bh, gap = 76, 36, 10
    total = 5*bw + 4*gap
    ox = (DW - total)/2
    boxes = [
        ("Binance\nExchange",  RUST,   "live exchange"),
        ("WebSocket\nClient",  BLUE,   "reconnects auto"),
        ("Book\nManager",      YELLOW, "state machine"),
        ("Order\nBook",        GREEN,  "BTreeMap"),
        ("HTTP API\n(Axum)",   PURPLE, "REST endpoints"),
    ]
    for i,(label,color,sub) in enumerate(boxes):
        x = ox + i*(bw+gap)
        draw_box(d, x, 22, bw, bh, DARK, color, label.split("\n")[0], color, sub)
        if "\n" in label:
            ds(d, x+bw/2, 22+bh/2+5, label.split("\n")[0], "Helvetica-Bold", 8, color, "middle")
            ds(d, x+bw/2, 22+bh/2-4, label.split("\n")[1], "Helvetica-Bold", 8, color, "middle")
            ds(d, x+bw/2, 22+bh/2-13, sub, "Helvetica", 6.5, MUTED, "middle")
        if i < 4:
            arrow_h(d, x+bw, 22+bh/2, x+bw+gap)
    return d

# ── Fig 2: Reconnect backoff timeline ─────────────────────────────────────────
def fig_backoff():
    d = Drawing(DW, 90)
    events = [
        (50,  "Connect", "FAIL", RUST),
        (150, "Wait 1s", "",     MUTED),
        (210, "Connect", "FAIL", RUST),
        (310, "Wait 2s", "",     MUTED),
        (380, "Connect", "FAIL", RUST),
        (450, "Wait 4s+\njitter", "", MUTED),
        (530, "Connect", "OK",   GREEN),
    ]
    d.add(Line(30, 52, DW-10, 52, strokeColor=BORDER, strokeWidth=1))
    for x, label, status, color in events:
        if x > DW-15: break
        r = 9 if status else 5
        fill = color if status else BORDER
        d.add(Circle(x, 52, r, fillColor=fill, strokeColor=WHITE, strokeWidth=0.8))
        if status:
            ds(d, x, 49, status[0], "Helvetica-Bold", 7, BG if status=="OK" else WHITE, "middle")
        lines = label.split("\n")
        for j,ln in enumerate(lines):
            ds(d, x, 66+j*9, ln, "Helvetica", 7, color, "middle")
        if status == "FAIL" and x < 500:
            next_x = [e[0] for e in events if e[0] > x][0]
            mid = (x + next_x)/2
            ds(d, mid, 38, [e[1] for e in events if e[0] > x][0],
               "Helvetica", 6.5, YELLOW, "middle")
    return d

# ── Fig 3: Channel architecture ────────────────────────────────────────────────
def fig_channel_arch():
    d = Drawing(DW, 100)
    bw, bh = 100, 32

    draw_box(d, 10,   54, bw, bh, DARK, BLUE,   "WebSocket Task",   BLUE,   "owns connection")
    draw_box(d, 155,  54, 110, bh, DARK, YELLOW, "mpsc channel",     YELLOW, "ordered queue")
    draw_box(d, 310,  54, bw, bh, DARK, GREEN,  "Manager Task",    GREEN,  "state machine")

    arrow_h(d, 110, 54+bh/2, 155, BLUE,   "WsEvent")
    arrow_h(d, 265, 54+bh/2, 310, YELLOW, "in order")

    # Book below manager
    draw_box(d, 310, 14, bw, 30, DARK, GREEN, "Order Book", GREEN, "Arc<RwLock<>>")
    arrow_v(d, 360, 54, 44, GREEN)

    # API readers
    draw_box(d, 430, 44, 80, 20, DARK, PURPLE, "API Handlers", PURPLE, None)
    d.add(Line(430, 54, 410, 39, strokeColor=PURPLE, strokeWidth=1, strokeDashArray=[3,2]))
    ds(d, 455, 68, "read-only", "Helvetica", 6.5, MUTED, "middle")

    ds(d, DW/2, 8, "One writer (manager). Many readers (API). Channel enforces ordering.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 4: Error taxonomy grid ────────────────────────────────────────────────
def fig_error_taxonomy():
    d = Drawing(DW, 140)
    errors = [
        ("WebSocket",   "Network dropped",           "Reconnect",             RUST,   10,  108),
        ("Parse",       "Unreadable JSON frame",      "Skip, keep going",      YELLOW, 10,  82),
        ("SequenceGap", "Missed an update",           "Rebuild book",          BLUE,   10,  56),
        ("SyncFailed",  "Bad first-connect handshake","Redo handshake",        MUTED,  10,  30),
        ("InvalidUrl",  "Bad config at startup",      "Crash immediately",     RUST,   DW/2+5, 108),
        ("InvalidField","Non-numeric price string",   "Skip frame",            YELLOW, DW/2+5, 82),
        ("Io",          "OS network error",           "Reconnect",             RUST,   DW/2+5, 56),
    ]
    col_w = DW/2 - 20
    for name, what, action, color, x, y in errors:
        d.add(Rect(x, y, col_w, 20, rx=3, ry=3, fillColor=DARK, strokeColor=color, strokeWidth=1.2))
        ds(d, x+6,      y+13, name,   "Helvetica-Bold", 8,   color)
        ds(d, x+6,      y+5,  what,   "Helvetica",      7,   MUTED)
        ds(d, x+col_w-6,y+5,  action, "Helvetica-Bold", 7,   color, "end")
    ds(d, DW/2, 14, "Each variant name tells you what happened. Each recovery action is different.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 5: WsEvent channel contract ───────────────────────────────────────────
def fig_wsevent():
    d = Drawing(DW, 80)
    hw = DW/2 - 15
    # Message variant
    d.add(Rect(10, 36, hw, 34, rx=4, ry=4, fillColor=GRN_BG, strokeColor=GREEN, strokeWidth=1.5))
    ds(d, 18, 64,  "Message(String)",               "Courier-Bold",   9, GREEN)
    ds(d, 18, 52,  "Raw JSON from Binance",         "Helvetica",      7.5, WHITE)
    ds(d, 18, 42,  '{"e":"depthUpdate","U":100...}', "Courier",        7, MUTED)

    # Reconnected variant
    d.add(Rect(10, 4, hw, 28, rx=4, ry=4, fillColor=YEL_BG, strokeColor=YELLOW, strokeWidth=1.5))
    ds(d, 18, 26, "Reconnected",                    "Courier-Bold",   9, YELLOW)
    ds(d, 18, 16, "No data. Signal only.",          "Helvetica",      7.5, WHITE)
    ds(d, 18, 8,  "Manager must discard + resync", "Helvetica",      7,   MUTED)

    # Arrow to manager box
    mx = hw + 30
    arrow_h(d, hw+12, 53, mx, GREEN,  "apply")
    arrow_h(d, hw+12, 18, mx, YELLOW, "resync")
    draw_box(d, mx, 8, 90, 52, DARK, BLUE, "Manager", BLUE, "decides what to do")
    return d

# ── Fig 6: Real order book ─────────────────────────────────────────────────────
def fig_order_book():
    d = Drawing(DW, 130)
    cw = DW/2 - 20
    rh = 13

    def col(ox, title, entries, color, bg):
        d.add(Rect(ox, 104, cw, 16, fillColor=color, strokeColor=WHITE, strokeWidth=0))
        ds(d, ox+cw/2, 109, title, "Helvetica-Bold", 8.5, BG, "middle")
        for i,(price,qty,best) in enumerate(entries):
            y = 90 - i*rh
            fill = colors.HexColor("#1a3a2a") if (best and color==GREEN) else \
                   colors.HexColor("#3a1a1a") if (best and color==RUST)  else bg
            bdr  = color if best else BORDER
            d.add(Rect(ox, y, cw, rh-1, fillColor=fill, strokeColor=bdr, strokeWidth=0.8))
            ds(d, ox+6,      y+4, price, "Helvetica", 8, color if best else MUTED)
            ds(d, ox+cw-6,   y+4, qty,   "Helvetica", 8, WHITE if best else MUTED, "end")
            if best:
                tag = "BEST BID" if color==GREEN else "BEST ASK"
                ds(d, ox+cw/2, y+4, tag, "Helvetica-Bold", 6.5, color, "middle")

    bids = [("29,501.00","0.842",True),("29,500.00","1.245",False),
            ("29,499.50","3.100",False),("29,498.00","0.500",False),("29,495.00","12.30",False)]
    asks = [("29,502.00","0.310",True),("29,503.00","2.100",False),
            ("29,504.50","0.750",False),("29,507.00","5.000",False),("29,510.00","8.200",False)]

    col(8,      "BIDS  (buyers, highest first)",  bids, GREEN, DARK)
    col(DW/2+8, "ASKS  (sellers, lowest first)", asks, RUST,  DARK)

    # Spread
    d.add(Line(8+cw, 97, DW/2+8, 97, strokeColor=YELLOW, strokeWidth=1.5))
    ds(d, DW/2, 100, "SPREAD = $1.00", "Helvetica-Bold", 7, YELLOW, "middle")
    ds(d, DW/2, 10,
       "Highest buyer at $29,501. Lowest seller at $29,502. Gap between them = spread.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 7: JSON anatomy ────────────────────────────────────────────────────────
def fig_json_anatomy():
    d = Drawing(DW, 150)
    lines = [
        ('{',                               WHITE),
        ('  "e": "depthUpdate",',           MUTED),
        ('  "E": 1699000000000,',           MUTED),
        ('  "s": "BTCUSDT",',              YELLOW),
        ('  "U": 100,',                    BLUE),
        ('  "u": 103,',                    BLUE),
        ('  "b": [["29500.00","1.245"],',   GREEN),
        ('         ["29499.50","0.000"]],', RUST),
        ('  "a": [["29501.00","0.310"]]',  GREEN),
        ('}',                               WHITE),
    ]
    box_w = 230
    lh = 12.5
    box_h = len(lines)*lh + 10
    d.add(Rect(8, 150-box_h, box_w, box_h, fillColor=DARK, strokeColor=BORDER, strokeWidth=1, rx=4, ry=4))
    for i,(_,_) in enumerate(lines):
        pass
    for i,(text,color) in enumerate(reversed(lines)):
        y = 150-box_h+6+i*lh
        ds(d, 14, y, text, "Courier", 7.5, color)

    annotations = [
        (1,  "event type",              MUTED),
        (2,  "timestamp (milliseconds)", MUTED),
        (3,  "trading symbol",           YELLOW),
        (4,  "first sequence ID",        BLUE),
        (5,  "last sequence ID",         BLUE),
        (6,  "bids to update",           GREEN),
        (7,  "qty=0 means DELETE",       RUST),
        (8,  "asks to update",           GREEN),
    ]
    ax = box_w + 20
    for line_i, label, color in annotations:
        ly = 150-box_h+6+(len(lines)-1-line_i)*lh + lh/2
        d.add(Line(box_w+8, ly, ax-2, ly, strokeColor=color, strokeWidth=0.7, strokeDashArray=[2,2]))
        ds(d, ax, ly-3, label, "Helvetica", 7, color)

    return d

# ── Fig 8: Zero-quantity deletion ─────────────────────────────────────────────
def fig_deletion():
    d = Drawing(DW, 100)
    cw = (DW-60)/2
    entries = [("29,501","0.842"),("29,500","1.245"),("29,499","3.100"),("29,498","0.500")]
    rh = 14

    def draw_col(ox, title, del_row=None):
        ds(d, ox+cw/2, 90, title, "Helvetica-Bold", 8.5, WHITE, "middle")
        for i,(p,q) in enumerate(entries):
            y = 74-i*rh
            removed = (i == del_row)
            fill   = RED_BG if removed else DARK
            border = RUST   if removed else BORDER
            d.add(Rect(ox, y, cw, rh-1, fillColor=fill, strokeColor=border, strokeWidth=1))
            ds(d, ox+6,      y+4, p, "Helvetica", 8, RUST if removed else GREEN)
            ds(d, ox+cw-6,   y+4, "REMOVED" if removed else q,
               "Helvetica", 8, RUST if removed else WHITE, "end")
            if removed:
                mid = y+(rh-1)/2
                d.add(Line(ox+4, mid, ox+cw-4, mid, strokeColor=RUST, strokeWidth=1.5))

    draw_col(8, "BEFORE update")
    draw_col(DW-cw-8, "AFTER update", del_row=2)

    # Update box in middle
    mx = DW/2-45
    d.add(Rect(mx, 40, 90, 30, fillColor=YEL_BG, strokeColor=YELLOW, strokeWidth=1.2, rx=3, ry=3))
    ds(d, mx+45, 64, "Update received:", "Helvetica-Bold", 7.5, YELLOW, "middle")
    ds(d, mx+45, 54, '["29499","0.00"]',  "Courier",        7.5, RUST,   "middle")
    ds(d, mx+45, 44, "qty=0 = DELETE",   "Helvetica",      7,   MUTED,  "middle")

    arrow_h(d, cw+8,    55, mx-2,    MUTED)
    arrow_h(d, mx+92,   55, DW-cw-8, MUTED)
    ds(d, DW/2, 8, "Binance signals deletion with quantity zero, not a separate message.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 9: Float vs Decimal ────────────────────────────────────────────────────
def fig_float_decimal():
    d = Drawing(DW, 95)
    hw = DW/2-12
    rows_float = [
        ("Binance sends:",  '"29431.50000000"',  WHITE, WHITE),
        ("Stored as float:", "29431.49999999996", WHITE, RUST),
        ("0.1 + 0.2 =",     "0.30000000000000004",WHITE,RUST),
        ("After 1000 ops:", "errors compound",   WHITE, RUST),
    ]
    rows_dec = [
        ("Binance sends:",   '"29431.50000000"', WHITE, WHITE),
        ("Stored exactly:",  "29431.50000000",   WHITE, GREEN),
        ("0.1 + 0.2 =",      "0.3",              WHITE, GREEN),
        ("After 1000 ops:", "still exact",       WHITE, GREEN),
    ]
    rh = 14
    for side,(rows,color,bg) in enumerate([(rows_float,RUST,RED_BG),(rows_dec,GREEN,GRN_BG)]):
        ox = 8 if side==0 else hw+20
        title = "f64 float  (WRONG)" if side==0 else "rust_decimal  (CORRECT)"
        d.add(Rect(ox, 14, hw, 72, fillColor=bg, strokeColor=color, strokeWidth=1.5, rx=4, ry=4))
        ds(d, ox+hw/2, 80, title, "Helvetica-Bold", 8.5, color, "middle")
        for i,(label,val,lc,vc) in enumerate(rows):
            y = 60-i*rh
            ds(d, ox+8,    y, label, "Helvetica", 7.5, MUTED)
            ds(d, ox+hw-8, y, val,   "Courier",   7,   vc,   "end")
    ds(d, DW/2, 4, "Floats store most decimals approximately. Financial software demands exact values.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 10: Two-layer parsing ──────────────────────────────────────────────────
def fig_two_layer():
    d = Drawing(DW, 100)
    hw = DW/2-30

    # Wire format box
    d.add(Rect(8, 42, hw, 48, fillColor=DARK, strokeColor=MUTED, strokeWidth=1, rx=3, ry=3))
    ds(d, hw/2+8, 84, "WIRE FORMAT", "Helvetica-Bold", 8, MUTED, "middle")
    ds(d, hw/2+8, 73, "private to binance.rs", "Helvetica", 7, MUTED, "middle")
    ds(d, 16, 62, 'RawDepthUpdate {', "Courier", 7.5, MUTED)
    ds(d, 16, 52, '  "U","u","b","a": strings', "Courier", 7,   MUTED)
    ds(d, 16, 44, '}',                  "Courier", 7.5, MUTED)

    # Arrow with label
    ax = hw+14
    d.add(Rect(ax, 58, 48, 18, fillColor=BLU_BG, strokeColor=BLUE, strokeWidth=1, rx=3, ry=3))
    ds(d, ax+24, 70, "parse()", "Courier-Bold", 7.5, BLUE, "middle")
    ds(d, ax+24, 60, "validate", "Helvetica",  6.5, MUTED, "middle")
    arrow_h(d, hw+8, 66, ax, MUTED)
    arrow_h(d, ax+48, 66, DW-hw-8, MUTED)

    # Domain type box
    d.add(Rect(DW-hw-6, 42, hw, 48, fillColor=GRN_BG, strokeColor=GREEN, strokeWidth=1.5, rx=3, ry=3))
    ds(d, DW-hw/2-6, 84, "DOMAIN TYPE", "Helvetica-Bold", 8, GREEN, "middle")
    ds(d, DW-hw/2-6, 73, "used everywhere else", "Helvetica", 7, GREEN, "middle")
    ds(d, DW-hw-1,  62, 'DepthUpdate {',          "Courier", 7.5, GREEN)
    ds(d, DW-hw-1,  52, '  first/last_update_id', "Courier", 7,   GREEN)
    ds(d, DW-hw-1,  44, '  bids/asks: Vec<Level>',  "Courier", 7, GREEN)

    # Change note
    d.add(Rect(8, 8, DW-16, 28, fillColor=YEL_BG, strokeColor=YELLOW, strokeWidth=1, rx=3, ry=3))
    ds(d, DW/2, 30, "Binance changes their field names tomorrow?", "Helvetica-Bold", 8, YELLOW, "middle")
    ds(d, DW/2, 18, "Change one line in binance.rs. Nothing else in the system knows.", "Helvetica", 7.5, WHITE, "middle")
    return d

# ── Fig 11: Sequence numbers ───────────────────────────────────────────────────
def fig_sequences():
    d = Drawing(DW, 100)
    bw, bh, gap = 85, 22, 8

    def draw_row(y, items, title, title_color):
        ds(d, 8, y+bh+6, title, "Helvetica-Bold", 8, title_color)
        for i,(U,u,ok) in enumerate(items):
            x = 8+i*(bw+gap)
            if x+bw > DW-8: break
            fill   = GRN_BG if ok else RED_BG
            border = GREEN  if ok else RUST
            d.add(Rect(x, y, bw, bh, fillColor=fill, strokeColor=border, strokeWidth=1.2, rx=2, ry=2))
            ds(d, x+bw/2, y+14, f"U={U}", "Courier", 8, border, "middle")
            ds(d, x+bw/2, y+4,  f"u={u}", "Courier", 8, WHITE,  "middle")
            if i < len(items)-1 and x+bw+gap < DW-8:
                next_ok = items[i+1][2]
                c = GREEN if (ok and next_ok) else RUST
                arrow_h(d, x+bw, y+bh/2, x+bw+gap, c)
                if not next_ok and ok:
                    ds(d, x+bw+gap/2, y+bh/2+5, "GAP!", "Helvetica-Bold", 6.5, RUST, "middle")

    good = [(100,103,True),(104,107,True),(108,110,True),(111,114,True)]
    bad  = [(100,103,True),(104,107,True),(109,112,False)]  # gap between 107 and 109
    draw_row(60, good, "No gaps: each U = previous u + 1", GREEN)
    draw_row(18, bad,  "Gap detected: expected U=108, got U=109", RUST)
    return d

# ── Fig 12: BTreeMap vs HashMap ────────────────────────────────────────────────
def fig_btree_vs_hash():
    d = Drawing(DW, 120)
    hw = DW/2-15
    rh = 12

    hashmap = [("29498","0.500"),("29501","0.842"),("29495","12.30"),("29500","1.245"),("29499","3.100")]
    btree   = [("29495","12.30"),("29498","0.500"),("29499","3.100"),("29500","1.245"),("29501","0.842")]

    for side,(data,color,title,note) in enumerate([
        (hashmap, RUST,  "HashMap  (unordered)", "To find best: scan ALL entries"),
        (btree,   GREEN, "BTreeMap  (sorted)",   "Best bid = last entry, always"),
    ]):
        ox = 8 if side==0 else hw+22
        d.add(Rect(ox, 18, hw, 96, fillColor=DARK, strokeColor=color, strokeWidth=1.5, rx=4, ry=4))
        ds(d, ox+hw/2, 109, title, "Helvetica-Bold", 8.5, color, "middle")
        for i,(p,q) in enumerate(data):
            y = 94-i*rh
            is_best = (side==1 and i==len(data)-1)
            fill   = GRN_BG if is_best else colors.HexColor("#0d1117")
            border = GREEN  if is_best else BORDER
            d.add(Rect(ox+6, y, hw-12, rh-1, fillColor=fill, strokeColor=border, strokeWidth=0.6))
            ds(d, ox+12,     y+3, p, "Courier", 7.5, GREEN if is_best else MUTED)
            ds(d, ox+hw-12,  y+3, q, "Courier", 7.5, WHITE if is_best else MUTED, "end")
            if is_best:
                ds(d, ox+hw/2+ox*0, y+3, "BEST BID", "Helvetica-Bold", 6.5, GREEN, "middle")
        d.add(Rect(ox+6, 20, hw-12, 12, fillColor=RED_BG if side==0 else GRN_BG,
                    strokeColor=color, strokeWidth=0.8))
        ds(d, ox+hw/2, 24, note, "Helvetica", 6.5, color, "middle")
    return d

# ── Fig 13: Book memory layout ─────────────────────────────────────────────────
def fig_book_memory():
    d = Drawing(DW, 120)
    cw = DW/2-20
    rh = 13
    bids = [("29,495","12.30"),("29,498","0.500"),("29,499","3.100"),("29,500","1.245"),("29,501","0.842")]
    asks = [("29,502","0.310"),("29,503","2.100"),("29,504","0.750"),("29,507","5.000"),("29,510","8.200")]

    for side,(data,color,ox,best_i,best_label,op) in enumerate([
        (bids, GREEN, 8,   4, "BEST BID", "next_back()"),
        (asks, RUST,  DW-cw-8, 0, "BEST ASK", "next()"),
    ]):
        ds(d, ox+cw/2, 112, f"{'bids' if side==0 else 'asks'} BTreeMap", "Helvetica-Bold", 8, color, "middle")
        ds(d, ox+cw/2, 102, "sorted low to high", "Helvetica", 7, MUTED, "middle")
        for i,(p,q) in enumerate(data):
            y = 88-i*rh
            is_best = (i==best_i)
            fill   = GRN_BG if (is_best and side==0) else RED_BG if (is_best and side==1) else DARK
            border = color  if is_best else BORDER
            d.add(Rect(ox, y, cw, rh-1, fillColor=fill, strokeColor=border, strokeWidth=1.2 if is_best else 0.5))
            ds(d, ox+6,    y+4, p, "Courier", 8, color if is_best else MUTED)
            ds(d, ox+cw-6, y+4, q, "Courier", 8, WHITE if is_best else MUTED, "end")
            if is_best:
                tag_x = ox-6 if side==0 else ox+cw+6
                anchor = "end" if side==0 else "start"
                ds(d, tag_x, y+4, f"{op} -> {best_label}", "Helvetica-Bold", 6.5, color, anchor)
    return d

# ── Fig 14: Apply update flowchart ────────────────────────────────────────────
def fig_apply_flow():
    d = Drawing(DW, 110)
    cx = DW/2

    # Incoming
    d.add(Rect(cx-120, 90, 240, 16, fillColor=BLU_BG, strokeColor=BLUE, strokeWidth=1.5, rx=3, ry=3))
    ds(d, cx, 101, 'Price level arrives:  ["29499.50",  "0.00000000"]', "Courier", 8, BLUE, "middle")
    arrow_v(d, cx, 90, 80, MUTED)

    # Decision diamond
    s = 14
    d.add(Polygon([cx,80+s, cx+s*2.2,80, cx,80-s, cx-s*2.2,80],
                   fillColor=DARKER, strokeColor=YELLOW, strokeWidth=1.5))
    ds(d, cx, 83, "quantity == 0?", "Helvetica-Bold", 8, YELLOW, "middle")
    ds(d, cx, 74, "is this a deletion?", "Helvetica", 7, MUTED, "middle")

    # YES left -> remove
    d.add(Line(cx-s*2.2, 80, 60, 80, strokeColor=RUST, strokeWidth=1.2))
    ds(d, (cx-s*2.2+60)/2, 84, "YES", "Helvetica-Bold", 7.5, RUST, "middle")
    arrow_v(d, 95, 80, 52, RUST)
    d.add(Rect(10, 34, 170, 18, fillColor=RED_BG, strokeColor=RUST, strokeWidth=1.5, rx=3, ry=3))
    ds(d, 95, 46, "map.remove(price)", "Courier-Bold", 9, RUST, "middle")
    ds(d, 95, 36, "price level deleted from book", "Helvetica", 7, MUTED, "middle")

    # NO right -> insert
    d.add(Line(cx+s*2.2, 80, DW-60, 80, strokeColor=GREEN, strokeWidth=1.2))
    ds(d, (cx+s*2.2+DW-60)/2, 84, "NO", "Helvetica-Bold", 7.5, GREEN, "middle")
    arrow_v(d, DW-95, 80, 52, GREEN)
    d.add(Rect(DW-180, 34, 170, 18, fillColor=GRN_BG, strokeColor=GREEN, strokeWidth=1.5, rx=3, ry=3))
    ds(d, DW-95, 46, "map.insert(price, qty)", "Courier-Bold", 9, GREEN, "middle")
    ds(d, DW-95, 36, "add new or replace existing", "Helvetica", 7, MUTED, "middle")

    ds(d, cx, 14, "Two paths only. The BTreeMap handles new vs existing automatically.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 15: Spread visual ──────────────────────────────────────────────────────
def fig_spread():
    d = Drawing(DW, 90)
    ax1, ax2 = 30, DW-20
    ay = 42
    d.add(Line(ax1, ay, ax2, ay, strokeColor=BORDER, strokeWidth=1))

    prices = [(0.08,"29495"),(0.22,"29498"),(0.38,"29501",True,False),
              (0.48,"29502",False,True),(0.62,"29504"),(0.78,"29507"),(0.92,"29510")]
    span = ax2-ax1

    bid_x = ask_x = 0
    for item in prices:
        frac = item[0]; label = item[1]
        is_bid = len(item)>2 and item[2]
        is_ask = len(item)>3 and item[3]
        x = ax1 + frac*span
        color = GREEN if is_bid else RUST if is_ask else BORDER
        h = 16 if (is_bid or is_ask) else 6
        d.add(Line(x, ay-h/2, x, ay+h/2, strokeColor=color, strokeWidth=2 if h>6 else 0.8))
        ds(d, x, ay-h/2-10, label, "Helvetica", 7, color, "middle")
        if is_bid: bid_x = x; ds(d, x, ay+h/2+4, "BEST BID", "Helvetica-Bold", 6.5, GREEN, "middle")
        if is_ask: ask_x = x; ds(d, x, ay+h/2+4, "BEST ASK", "Helvetica-Bold", 6.5, RUST, "middle")

    if bid_x and ask_x:
        bry = ay+26
        d.add(Line(bid_x, bry, ask_x, bry, strokeColor=YELLOW, strokeWidth=2))
        d.add(Line(bid_x, bry-4, bid_x, bry+4, strokeColor=YELLOW, strokeWidth=1.5))
        d.add(Line(ask_x, bry-4, ask_x, bry+4, strokeColor=YELLOW, strokeWidth=1.5))
        ds(d, (bid_x+ask_x)/2, bry+6, "SPREAD = $1.00", "Helvetica-Bold", 7.5, YELLOW, "middle")

    ds(d, DW/2, 4, "The spread is the cost of buying then immediately selling. Tight spread = liquid market.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 16: Manager state machine ─────────────────────────────────────────────
def fig_state_machine():
    d = Drawing(DW, 120)
    bw, bh = 115, 42
    gap = (DW - 3*bw) / 4
    states = [
        (gap,          "FETCHING",    "REST snapshot\nbeing requested.\nEvents queue up.", BLUE),
        (gap*2+bw,     "HANDSHAKING", "Snapshot arrived.\nSearching for first\nvalid event.", YELLOW),
        (gap*3+2*bw,   "LIVE",        "Applying events.\nChecking sequence\ncontinuity.", GREEN),
    ]
    cx_list = []
    for bx,title,desc,color in states:
        cx = bx+bw/2; cx_list.append((cx, bx, bh))
        d.add(Rect(bx, 52, bw, bh, rx=6, ry=6, fillColor=DARK, strokeColor=color, strokeWidth=2))
        ds(d, cx, 52+bh-10, title, "Helvetica-Bold", 9, color, "middle")
        for j,ln in enumerate(desc.split("\n")):
            ds(d, cx, 52+bh-22-j*9, ln, "Helvetica", 7, MUTED, "middle")

    # Forward arrows
    for i in range(len(states)-1):
        cx1 = cx_list[i][0]+bw/2
        cx2 = cx_list[i+1][1]
        labels = ["snapshot arrives", "first valid event"]
        arrow_h(d, cx1, 73, cx2, [BLUE,YELLOW][i], labels[i])

    # Reset arc (back to start from any state)
    reset_y = 44
    for cx,bx,_ in cx_list[1:]:
        d.add(Line(cx, 52, cx, reset_y, strokeColor=RUST, strokeWidth=1, strokeDashArray=[3,2]))
    d.add(Line(cx_list[1][0], reset_y, cx_list[0][1]+bw/2, reset_y,
                strokeColor=RUST, strokeWidth=1, strokeDashArray=[3,2]))
    arrow_h(d, cx_list[0][0], reset_y, cx_list[0][1], RUST)
    ds(d, DW/2, 36, "Any failure from any state: clear book, go back to FETCHING",
       "Helvetica", 7, RUST, "middle")

    ds(d, DW/2, 10, "Reconnect signal, sequence gap, or failed handshake all lead to the same recovery.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 17: Sync timeline ──────────────────────────────────────────────────────
def fig_sync_timeline():
    d = Drawing(DW, 100)
    t0, t1 = 20, DW-20
    ay = 55
    span = t1-t0
    snap_x = t0 + span*0.45

    d.add(Line(t0, ay, t1, ay, strokeColor=BORDER, strokeWidth=1))

    # REST bar above axis
    d.add(Rect(t0, ay+8, snap_x-t0, 16, fillColor=BLU_BG, strokeColor=BLUE, strokeWidth=1.5))
    ds(d, (t0+snap_x)/2, ay+19, "REST request in flight (~200ms)", "Helvetica", 7.5, BLUE, "middle")

    # WS bar below axis
    d.add(Rect(t0, ay-22, t1-t0, 12, fillColor=GRN_BG, strokeColor=GREEN, strokeWidth=1))
    ds(d, (t0+t1)/2, ay-13, "WebSocket events arriving continuously", "Helvetica", 7.5, GREEN, "middle")

    # Snapshot arrival marker
    d.add(Line(snap_x, ay-28, snap_x, ay+26, strokeColor=YELLOW, strokeWidth=2, strokeDashArray=[4,2]))
    ds(d, snap_x, ay+30, "Snapshot", "Helvetica-Bold", 7, YELLOW, "middle")
    ds(d, snap_x, ay+38, "arrives", "Helvetica-Bold", 7, YELLOW, "middle")

    # Zone labels
    ds(d, (t0+snap_x)/2, ay-34, "STALE ZONE", "Helvetica-Bold", 7, RUST, "middle")
    ds(d, (t0+snap_x)/2, ay-43, "may already be in snapshot", "Helvetica", 6.5, MUTED, "middle")
    ds(d, (snap_x+t1)/2, ay-34, "FRESH ZONE", "Helvetica-Bold", 7, GREEN, "middle")
    ds(d, (snap_x+t1)/2, ay-43, "happened after snapshot", "Helvetica", 6.5, MUTED, "middle")

    ds(d, DW/2, 6, "The snapshot captures one moment. Events before that moment are already inside it.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 18: Event queue at handshake ──────────────────────────────────────────
def fig_event_queue():
    d = Drawing(DW, 110)
    rh = 14
    snap_id = 1000
    d.add(Rect(DW/2-90, 94, 180, 14, fillColor=BLU_BG, strokeColor=BLUE, strokeWidth=1.5, rx=3, ry=3))
    ds(d, DW/2, 104, f"Snapshot lastUpdateId = {snap_id}", "Courier-Bold", 8.5, BLUE, "middle")

    events = [
        (980, 995,  "STALE",   RUST,  "u<=1000: already in snapshot"),
        (996, 1000, "STALE",   RUST,  "u<=1000: already in snapshot"),
        (1001,1004, "FIRST OK",GREEN, "U<=1001 and u>=1001: APPLY"),
        (1005,1008, "PENDING", MUTED, "applied next in live mode"),
        (1009,1012, "PENDING", MUTED, "applied next in live mode"),
    ]
    for i,(Uv,uv,label,color,note) in enumerate(events):
        y = 78-i*rh
        fill = RED_BG if color==RUST else GRN_BG if color==GREEN else DARK
        border = color if color != MUTED else BORDER
        d.add(Rect(8, y, DW-16, rh-1, fillColor=fill, strokeColor=border, strokeWidth=1.2 if color!=MUTED else 0.5))
        ds(d, 14,      y+4, f"U={Uv}  u={uv}", "Courier",       8, color)
        ds(d, DW/2,    y+4, label,               "Helvetica-Bold",7.5, color, "middle")
        ds(d, DW-14,   y+4, note,                "Helvetica",     6.5, MUTED, "end")

    ds(d, DW/2, 4, "Stale events are discarded. First event where U<=lastId+1 AND u>lastId is the starting point.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 19: Handshake decision ────────────────────────────────────────────────
def fig_handshake():
    d = Drawing(DW, 130)
    cx = DW/2

    # Input
    d.add(Rect(cx-130, 114, 260, 14, fillColor=BLU_BG, strokeColor=BLUE, strokeWidth=1.5, rx=3, ry=3))
    ds(d, cx, 124, "Event: U=1001, u=1004   |   snapshot_id=1000", "Courier", 8, BLUE, "middle")
    arrow_v(d, cx, 114, 104, MUTED)

    # Check 1
    s1 = 12
    d.add(Polygon([cx,104+s1, cx+s1*2.3,104, cx,104-s1, cx-s1*2.3,104],
                   fillColor=DARKER, strokeColor=YELLOW, strokeWidth=1.5))
    ds(d, cx, 107, "u <= snapshot_id?", "Helvetica-Bold", 8, YELLOW, "middle")
    ds(d, cx, 99,  "Is this event stale?", "Helvetica", 6.5, MUTED, "middle")

    d.add(Line(cx+s1*2.3, 104, DW-20, 104, strokeColor=RUST, strokeWidth=1.2))
    ds(d, cx+s1*2.3+25, 108, "YES = DISCARD", "Helvetica-Bold", 7.5, RUST)
    arrow_v(d, cx, 104-s1, 80, MUTED)
    ds(d, cx+6, 92, "NO", "Helvetica-Bold", 7.5, MUTED)

    # Check 2
    s2 = 12
    d.add(Polygon([cx,80+s2, cx+s2*2.3,80, cx,80-s2, cx-s2*2.3,80],
                   fillColor=DARKER, strokeColor=YELLOW, strokeWidth=1.5))
    ds(d, cx, 83, "U > snapshot_id+1?",  "Helvetica-Bold", 8, YELLOW, "middle")
    ds(d, cx, 75, "Is there a gap?",     "Helvetica", 6.5, MUTED, "middle")

    d.add(Line(cx+s2*2.3, 80, DW-20, 80, strokeColor=RUST, strokeWidth=1.2))
    ds(d, cx+s2*2.3+25, 84, "YES = RESYNC", "Helvetica-Bold", 7.5, RUST)
    arrow_v(d, cx, 80-s2, 52, GREEN)
    ds(d, cx+6, 68, "NO", "Helvetica-Bold", 7.5, GREEN)

    d.add(Rect(cx-110, 38, 220, 14, fillColor=GRN_BG, strokeColor=GREEN, strokeWidth=1.5, rx=3, ry=3))
    ds(d, cx, 48, "APPLY  -  this is the starting point", "Helvetica-Bold", 8.5, GREEN, "middle")

    ds(d, cx, 20, "U=1001 <= 1001 (snapshot+1) AND u=1004 > 1000  ->  valid first event.", "Helvetica", 7.5, WHITE, "middle")
    ds(d, cx, 10, "Every event passes two checks. Only one outcome leads to applying it.", "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 20: Sequence continuity ────────────────────────────────────────────────
def fig_continuity():
    d = Drawing(DW, 100)
    bw, bh, gap = 82, 22, 8

    def row(y, items, title, tc):
        ds(d, 8, y+bh+8, title, "Helvetica-Bold", 8, tc)
        for i,(U,u,ok) in enumerate(items):
            x = 8+i*(bw+gap)
            if x+bw > DW-8: break
            fill   = GRN_BG if ok else RED_BG
            border = GREEN  if ok else RUST
            d.add(Rect(x, y, bw, bh, fillColor=fill, strokeColor=border, strokeWidth=1.2, rx=2, ry=2))
            ds(d, x+bw/2, y+14, f"U={U}", "Courier", 8, border, "middle")
            ds(d, x+bw/2, y+4,  f"u={u}", "Courier", 8, WHITE,  "middle")
            if i < len(items)-1 and x+bw+gap+bw <= DW-8:
                nxt = items[i+1]
                c = GREEN if (ok and nxt[2]) else RUST
                arrow_h(d, x+bw, y+bh/2, x+bw+gap, c)
                if ok and not nxt[2]:
                    ds(d, x+bw+gap/2, y+bh/2+6, "GAP", "Helvetica-Bold", 6.5, RUST, "middle")

    row(62, [(100,103,True),(104,107,True),(108,110,True),(111,114,True)],
        "GOOD: each U = previous u + 1", GREEN)
    row(20, [(100,103,True),(104,107,True),(109,112,False)],
        "BAD: expected U=108, got U=109 - one update was lost", RUST)
    return d

# ── Fig 21: RwLock readers/writer ─────────────────────────────────────────────
def fig_rwlock():
    d = Drawing(DW, 110)
    bk_x, bk_y, bk_w, bk_h = DW/2-55, 36, 110, 36

    # Book
    d.add(Rect(bk_x, bk_y, bk_w, bk_h, fillColor=DARK, strokeColor=GREEN, strokeWidth=2, rx=5, ry=5))
    ds(d, bk_x+bk_w/2, bk_y+24, "Order Book", "Helvetica-Bold", 9, GREEN, "middle")
    ds(d, bk_x+bk_w/2, bk_y+10, "Arc<RwLock<OrderBook>>", "Courier", 6.5, MUTED, "middle")

    # Manager (writer) left
    d.add(Rect(10, 46, 95, 24, fillColor=DARK, strokeColor=YELLOW, strokeWidth=1.5, rx=3, ry=3))
    ds(d, 57, 62, "Manager", "Helvetica-Bold", 9, YELLOW, "middle")
    ds(d, 57, 51, "apply(update)", "Courier", 7, MUTED, "middle")
    arrow_h(d, 105, 58, bk_x, YELLOW, "WRITE")

    # API readers (right)
    readers = [("GET /book/best",75),("GET /book/spread",54),("GET /book/bids",33)]
    for label,y in readers:
        rx2 = DW-100
        d.add(Rect(rx2, y, 88, 15, fillColor=DARK, strokeColor=BLUE, strokeWidth=1, rx=2, ry=2))
        ds(d, rx2+44, y+6, label, "Helvetica", 7.5, BLUE, "middle")
        d.add(Line(rx2, y+7, bk_x+bk_w, bk_y+bk_h/2, strokeColor=BLUE, strokeWidth=0.8, strokeDashArray=[3,2]))

    ds(d, DW-56, 96, "READ (concurrent)", "Helvetica-Bold", 7, BLUE, "middle")
    ds(d, 57, 96, "WRITE (exclusive)", "Helvetica-Bold", 7, YELLOW, "middle")

    ds(d, DW/2, 16, "Many readers can run at once. The writer waits for all readers to finish.", "Helvetica", 8, WHITE, "middle")
    ds(d, DW/2, 6,  "std::sync::RwLock is correct here because book writes are pure computation - no awaiting.", "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 22: API endpoints ─────────────────────────────────────────────────────
def fig_api_endpoints():
    d = Drawing(DW, 135)
    endpoints = [
        ("GET", "/health",        "Is the book synced? Returns 503 until first snapshot applied.",    BLUE),
        ("GET", "/book/best",     "Best bid price, best ask price, spread, and last update ID.",       GREEN),
        ("GET", "/book/snapshot", "Top N levels on both sides (default 20, max 100).",                 GREEN),
        ("GET", "/book/bids",     "Top N bid levels only. Pass ?depth=50 for more.",                   GREEN),
        ("GET", "/book/asks",     "Top N ask levels only. Pass ?depth=50 for more.",                   GREEN),
        ("GET", "/book/spread",   "The current spread as a single decimal number.",                    YELLOW),
    ]
    rh = 18
    for i,(method,path,desc,color) in enumerate(endpoints):
        y = 118-i*rh
        d.add(Rect(8, y, DW-16, rh-1, fillColor=DARK, strokeColor=BORDER, strokeWidth=0.5, rx=2, ry=2))
        d.add(Rect(8, y, 32, rh-1, fillColor=color, strokeColor=color, strokeWidth=0, rx=2, ry=2))
        ds(d, 24,   y+6, method, "Helvetica-Bold", 7.5, BG, "middle")
        ds(d, 50,   y+6, path,   "Courier-Bold",   8,   color)
        ds(d, 160,  y+6, desc,   "Helvetica",       7,   MUTED)
    ds(d, DW/2, 6, "All endpoints return JSON. No authentication - this is an internal service.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 23: HTTP request flow ──────────────────────────────────────────────────
def fig_http_flow():
    d = Drawing(DW, 90)
    steps = [
        ("Client",      "HTTP GET\n/book/best",    BLUE),
        ("Axum\nRouter", "match route\nextract state", PURPLE),
        ("Handler",     "acquire\nread lock",      YELLOW),
        ("OrderBook",   "top_bids()\nbest_ask()",  GREEN),
        ("Response",    "JSON\n200 OK",             BLUE),
    ]
    bw = min(75, (DW-20)/len(steps) - 10)
    gap = (DW - 20 - len(steps)*bw) / (len(steps)-1)
    for i,(title,action,color) in enumerate(steps):
        x = 10+i*(bw+gap)
        if x+bw > DW-5: break
        draw_box(d, x, 36, bw, 36, DARK, color, title.split("\n")[0], color, None)
        if "\n" in title:
            ds(d, x+bw/2, 36+28, title.split("\n")[0], "Helvetica-Bold", 7.5, color, "middle")
            ds(d, x+bw/2, 36+18, title.split("\n")[1], "Helvetica-Bold", 7.5, color, "middle")
        for j,ln in enumerate(action.split("\n")):
            ds(d, x+bw/2, 36+10-j*9, ln, "Helvetica", 6.5, MUTED, "middle")
        if i < len(steps)-1 and x+bw+gap+bw <= DW-5:
            arrow_h(d, x+bw, 54, x+bw+gap, MUTED)

    ds(d, DW/2, 14, "The handler acquires a read lock, reads the book, releases the lock, serializes to JSON.",
       "Helvetica", 7.5, WHITE, "middle")
    ds(d, DW/2, 5,  "Lock is held for microseconds. Many requests can run simultaneously.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 24: Config flow ────────────────────────────────────────────────────────
def fig_config():
    d = Drawing(DW, 90)
    bh = 14
    env_vars = [
        ("SYMBOL=BTCUSDT",        GREEN),
        ("API_PORT=3000",          BLUE),
        ("WS_BASE=wss://...",      YELLOW),
        ("REST_BASE=https://...",  YELLOW),
        ("CHANNEL_BUFFER=10000",   PURPLE),
        ("RUST_LOG=info",          MUTED),
    ]
    col_w = 155
    for i,(var,color) in enumerate(env_vars):
        row = i % 3; col_n = i // 3
        x = 8 + col_n*(col_w+8); y = 56-row*bh
        d.add(Rect(x, y, col_w, bh-1, fillColor=DARK, strokeColor=color, strokeWidth=0.8, rx=2, ry=2))
        ds(d, x+6, y+4, var, "Courier", 7.5, color)

    # Arrow to Config struct
    arrow_h(d, 8+col_w*2+8+8, 48, DW-150, MUTED, "from_env()")
    d.add(Rect(DW-148, 32, 140, 40, fillColor=DARK, strokeColor=WHITE, strokeWidth=1.5, rx=4, ry=4))
    ds(d, DW-78, 66, "Config { .. }", "Courier-Bold", 9, WHITE, "middle")
    ds(d, DW-78, 54, "typed, validated,",  "Helvetica", 7, MUTED, "middle")
    ds(d, DW-78, 44, "one place to change", "Helvetica", 7, MUTED, "middle")
    ds(d, DW-78, 34, "has sensible defaults", "Helvetica", 7, MUTED, "middle")

    ds(d, DW/2, 10, "Environment variables are the standard way to configure containerized services.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 25: Full wiring diagram ────────────────────────────────────────────────
def fig_full_wiring():
    d = Drawing(DW, 160)

    # Config at top
    d.add(Rect(DW/2-65, 138, 130, 18, fillColor=DARK, strokeColor=WHITE, strokeWidth=1.5, rx=3, ry=3))
    ds(d, DW/2, 150, "Config::from_env()", "Courier-Bold", 8, WHITE, "middle")

    # main.rs box
    d.add(Rect(DW/2-55, 106, 110, 24, fillColor=BLU_BG, strokeColor=BLUE, strokeWidth=2, rx=4, ry=4))
    ds(d, DW/2, 122, "#[tokio::main]", "Courier-Bold", 8, BLUE, "middle")
    ds(d, DW/2, 111, "main()", "Helvetica-Bold", 8, BLUE, "middle")
    arrow_v(d, DW/2, 138, 130, MUTED)

    # mpsc channel
    d.add(Rect(DW/2-50, 82, 100, 16, fillColor=YEL_BG, strokeColor=YELLOW, strokeWidth=1.2, rx=2, ry=2))
    ds(d, DW/2, 93, "mpsc::channel(10_000)", "Courier", 7.5, YELLOW, "middle")
    arrow_v(d, DW/2, 106, 98, MUTED)

    # Arc<RwLock<OrderBook>>
    d.add(Rect(DW/2-60, 58, 120, 16, fillColor=GRN_BG, strokeColor=GREEN, strokeWidth=1.5, rx=2, ry=2))
    ds(d, DW/2, 69, "Arc<RwLock<OrderBook>>", "Courier", 7.5, GREEN, "middle")

    # WS Client (left)
    draw_box(d, 10, 30, 100, 26, DARK, BLUE, "WS Client", BLUE, "tokio::spawn")
    arrow_h(d, 110, 43, DW/2-50, YELLOW, "tx (sender)")

    # Manager (right of center)
    draw_box(d, DW/2+52, 30, 100, 26, DARK, YELLOW, "Manager", YELLOW, "tokio::spawn")
    arrow_h(d, DW/2+50, 66, DW/2+52+50, GREEN, "Arc::clone")
    d.add(Line(DW/2+50, 90, DW/2+52, 43, strokeColor=YELLOW, strokeWidth=1, strokeDashArray=[3,2]))

    # Axum API (far right)
    draw_box(d, DW-110, 30, 100, 26, DARK, PURPLE, "Axum API", PURPLE, "axum::serve")
    arrow_h(d, DW/2+50, 66, DW-110, PURPLE, "Arc::clone")

    ds(d, DW/2, 12, "main() creates everything, wires it together, then serves HTTP until process ends.",
       "Helvetica", 7.5, WHITE, "middle")
    ds(d, DW/2, 4,  "The book is the only shared state. Everyone else communicates through it or the channel.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 26: Dockerfile layers ─────────────────────────────────────────────────
def fig_dockerfile():
    d = Drawing(DW, 120)
    layers = [
        ("Stage 1: Builder (rust:1.78-slim)",  [
            "FROM rust:1.78-slim AS builder",
            "COPY Cargo.toml Cargo.lock ./",
            "RUN cargo build --release (deps cached)",
            "COPY src ./src",
            "RUN cargo build --release (app only)",
        ], BLUE, BLU_BG),
        ("Stage 2: Runtime (distroless)",  [
            "FROM gcr.io/distroless/cc-debian12",
            "COPY --from=builder /target/release/crypto-orderbook .",
            "ENV SYMBOL=BTCUSDT API_PORT=3000",
            "EXPOSE 3000",
            "CMD [\"./crypto-orderbook\"]",
        ], GREEN, GRN_BG),
    ]
    hw = DW/2-12
    for side,(title,lines,color,bg) in enumerate(layers):
        ox = 8 if side==0 else hw+18
        d.add(Rect(ox, 20, hw, 92, fillColor=bg, strokeColor=color, strokeWidth=1.5, rx=4, ry=4))
        ds(d, ox+hw/2, 107, title, "Helvetica-Bold", 7.5, color, "middle")
        for j,ln in enumerate(lines):
            ds(d, ox+8, 90-j*13, ln, "Courier", 6.5, color if j==0 else MUTED)

    arrow_h(d, hw+8, 66, hw+18, MUTED, "binary only")
    ds(d, DW/2, 10, "Builder image ~1.5GB. Runtime image ~20MB. Only the compiled binary crosses stages.",
       "Helvetica", 7, MUTED, "middle")
    return d

# ── Fig 27: Production roadmap ─────────────────────────────────────────────────
def fig_roadmap():
    d = Drawing(DW, 200)
    items = [
        ("WebSocket Server API",   "Stream live book updates to subscribers in real time,\nnot just REST snapshots.",       RUST,   8,   170),
        ("Prometheus Metrics",     "Count events/sec, measure update latency, alert on gaps.\nNumbers before you optimize.", YELLOW, 8,   138),
        ("Rate Limiter",           "Token bucket on REST calls. Binance bans clients\nthat hammer the endpoint on failure.", BLUE,   8,   106),
        ("Circuit Breaker",        "After N failures, stop retrying and surface an alert.\nFailing fast beats silent loops.", GREEN, 8,    74),
        ("TimescaleDB",            "Store every tick update with a timestamp. Query\nhistorical spreads, VWAP, volatility.", PURPLE, 8,   42),
        ("Multi-Venue",            "Abstract the exchange layer so Coinbase and Kraken\nplug in alongside Binance.",          MUTED,  8,   10),
        ("VWAP Engine",            "Volume-weighted average price over sliding windows.\nCore risk metric for trading.",      RUST,   DW/2+4, 170),
        ("Risk Engine",            "Liquidation triggers, position limits, margin checks.\nAll require a correct book.",     YELLOW, DW/2+4, 138),
        ("Graceful Shutdown",      "Drain the channel, wait for in-flight writes, then\nstop. No lost updates on deploy.",   BLUE,   DW/2+4, 106),
        ("Event Sourcing",         "Store every depth update as an immutable log.\nReplay history, audit any book state.",   GREEN,  DW/2+4, 74),
        ("Docker Compose",         "Spin up engine + TimescaleDB + Prometheus + Grafana\nlocally with one command.",          PURPLE, DW/2+4, 42),
        ("Load Testing",           "Simulate 100k events/sec. Measure where the system\nbends before it breaks in prod.",    MUTED,  DW/2+4, 10),
    ]
    col_w = DW/2-16
    for title,desc,color,x,y in items:
        d.add(Rect(x, y, col_w, 26, fillColor=DARK, strokeColor=color, strokeWidth=1.2, rx=3, ry=3))
        ds(d, x+8,  y+20, title, "Helvetica-Bold", 8.5, color)
        for j,ln in enumerate(desc.split("\n")):
            ds(d, x+8, y+11-j*8, ln, "Helvetica", 6.5, MUTED)
    return d


# ==============================================================================
#  CONTENT
# ==============================================================================

def build():
    story = []

    # ── Cover ──────────────────────────────────────────────────────────────────
    story += [SP(50),
        P("Crypto Order Book Engine", "title"),
        P("System Design and Architecture", "subtitle"),
        HR(),
        P("A production-grade real-time order book built in Rust. "
          "WebSocket ingestion from Binance, async Tokio throughout, "
          "REST API with Axum. Every decision documented. Every tradeoff named.", "body"),
        P("Language: Rust  |  Runtime: Tokio  |  Exchange: Binance  |  API: Axum", "body2"),
        PageBreak(),
    ]

    # ── Part 1: What We're Building ────────────────────────────────────────────
    story += [
        P("Part 1 - What We Are Actually Building", "h1"), HR(),
        P("Binance is a crypto exchange. When people buy and sell Bitcoin, those orders "
          "sit in a list called an order book. Bids on one side are people wanting to buy "
          "and what they are willing to pay. Asks on the other side are people wanting to "
          "sell and what price they want. That list changes thousands of times per second.", "body"),
        P("We are building a program that connects to Binance, receives those changes in "
          "real time, keeps its own copy of the order book in memory always up to date, "
          "and lets other programs ask what the order book looks like right now over HTTP.", "body"),
        SP(8), fig_system_overview(), SP(4),
        CAP("Figure 1 - Top-level data flow. Each box is an independent piece. Arrows show how data moves."),
        PageBreak(),
    ]

    # ── Part 2: Dependencies ───────────────────────────────────────────────────
    story += [
        P("Part 2 - Cargo.toml: Every Dependency and Why", "h1"), HR(),
        P("Every dependency is a bet. You are betting this tool is mature, maintained, "
          "and will not fight with the rest of the system. Here is every bet we made.", "body"),
        P("tokio", "h2"),
        P("Rust by default does one thing at a time. If you are waiting for Binance to "
          "send a message, your program just sits there doing nothing. Tokio lets your "
          "program do many things at the same time: listen to Binance, handle an incoming "
          "HTTP request, run a timer, all on the same thread without blocking. "
          "Everything else depends on this being here.", "body"),
        P("tokio-tungstenite", "h2"),
        P("Binance sends us data over a WebSocket, which is like a phone call you open "
          "once and both sides can talk back and forth as long as they want. "
          "This library speaks that protocol and is built to work with Tokio natively, "
          "so they share the same plumbing underneath.", "body"),
        P("serde and serde_json", "h2"),
        P("Binance sends JSON text. Serde reads that text and fills in our Rust structs "
          "automatically. You describe the shape you expect and it handles the rest. "
          "The standard for JSON in Rust.", "body"),
        P("rust_decimal", "h2"),
        P("Binance sends prices as strings like \"29431.50000000\". Convert that to a "
          "regular float and you get silent rounding errors. 0.1 plus 0.2 in floating "
          "point equals 0.30000000000000004, not 0.3. In financial software that error "
          "compounds with every operation. rust_decimal stores the number exactly as "
          "written. We never use floats for prices.", "body"),
        SP(6), fig_float_decimal(), SP(4),
        CAP("Figure 2 - Float math is approximate. Decimal math is exact. This is not optional in finance."),
        P("thiserror", "h2"),
        P("When something goes wrong, the response depends on what went wrong. "
          "Network dropped? Reconnect. Bad JSON? Skip that frame. Missed a sequence "
          "number? Throw everything away and rebuild. thiserror lets us name each "
          "failure type so the code that catches it knows exactly what to do.", "body"),
        P("axum", "h2"),
        P("The HTTP server that handles incoming requests for book data. Built by the "
          "same team as Tokio so they work together natively without fighting.", "body"),
        P("tracing and tracing-subscriber", "h2"),
        P("Our program does many things simultaneously. A regular print statement has no "
          "way to tell you which task it came from. tracing attaches context to every "
          "log message: which connection, which task, what it was doing. "
          "Essential when debugging a system under load.", "body"),
        P("reqwest", "h2"),
        P("HTTP client for fetching the order book snapshot from Binance's REST API. "
          "Built on the same network stack as Axum so they share connection infrastructure.", "body"),
        SP(6), tradeoff_table([
            ["Price type",     "rust_decimal",   "f64 float",   "Floats round silently. Financial systems cannot have invisible rounding errors."],
            ["HTTP framework",  "axum",           "actix-web",   "actix-web uses its own actor runtime that conflicts with Tokio. axum is Tokio-native."],
            ["Error handling",  "thiserror",      "anyhow",      "anyhow erases error types. We need to match on variants to choose the right recovery."],
            ["Logging",         "tracing",        "println",     "println has no concept of async task context. tracing tracks which task logged what."],
        ]),
        PageBreak(),
    ]

    # ── Part 3: Error Taxonomy ─────────────────────────────────────────────────
    story += [
        P("Part 3 - error.rs: The Failure Taxonomy", "h1"), HR(),
        P("We defined all error types before writing any logic. "
          "If you define what can go wrong first, every piece of code knows exactly "
          "what failures it is allowed to produce. If you do it later, you get a "
          "scattered mess of inconsistent errors with no clear recovery story.", "body"),
        SP(6), fig_error_taxonomy(), SP(4),
        CAP("Figure 3 - Each error type maps to a specific recovery action. They are not interchangeable."),
        P("WebSocket", "h2"),
        P("The connection to Binance dropped. This is a network problem, not a data problem. "
          "Recovery: close the connection and open a new one. The book state is still valid "
          "as long as we can pick up from where we left off.", "body"),
        P("Parse", "h2"),
        P("Binance sent us a JSON message we could not understand. One bad frame does not "
          "mean the stream is broken. Recovery: log the message, skip it, keep going.", "body"),
        P("SequenceGap", "h2"),
        P("We received update number 100 and then update number 102, skipping 101. "
          "That missing update could be anything. We cannot reconstruct what changed. "
          "Recovery: throw away the entire book and rebuild it from a fresh snapshot.", "body"),
        P("SyncFailed", "h2"),
        P("When we first connect there is a handshake that aligns our book with Binance. "
          "If that handshake fails the numbers do not add up. "
          "Recovery: start the handshake over.", "body"),
        P("InvalidUrl", "h2"),
        P("The address we are trying to connect to is malformed. This only happens at startup "
          "with a bad configuration. Recovery: crash immediately with a clear message. "
          "There is no recovering from bad config, a human needs to fix it.", "body"),
        PageBreak(),
    ]

    # ── Part 4: WsEvent ────────────────────────────────────────────────────────
    story += [
        P("Part 4 - ws/mod.rs: The Channel Contract", "h1"), HR(),
        P("The WebSocket client and the order book manager are two separate tasks that "
          "never call each other directly. They communicate through a queue. "
          "WsEvent defines everything that can ever go in that queue. "
          "It is the contract between them.", "body"),
        SP(6), fig_wsevent(), SP(4),
        CAP("Figure 4 - Two variants, two completely different responses from the manager."),
        P("Message(String)", "h2"),
        P("Binance sent us something. Here is the raw text exactly as received. "
          "We do not parse it here because the WebSocket client's only job is transport. "
          "If Binance changes their JSON format, only one file needs updating.", "body"),
        P("Reconnected", "h2"),
        P("We lost the connection and re-established it. Everything the manager knew "
          "before is potentially from a dead stream. This signal goes into the queue "
          "before any new messages so the manager sees it first and resets before "
          "processing anything from the new connection.", "body"),
        PageBreak(),
    ]

    # ── Part 5: WS Client ─────────────────────────────────────────────────────
    story += [
        P("Part 5 - ws/client.rs: The Reconnecting WebSocket Client", "h1"), HR(),
        P("This piece talks to Binance. It runs forever in the background maintaining "
          "the connection and feeding messages into the queue. The entire rest of the "
          "system never touches a socket.", "body"),
        SP(6), fig_backoff(), SP(4),
        CAP("Figure 5 - Each failure doubles the wait time. Jitter staggers simultaneous reconnects."),
        P("Why it runs in its own task", "h2"),
        P("When we call spawn, we hand the connection off to a background process and "
          "walk away. It runs forever feeding messages. If we did not do this, "
          "listening to Binance would block everything else in the program.", "body"),
        P("Why the wait time grows after each failure", "h2"),
        P("Imagine Binance's servers go down. If every client immediately retries every "
          "millisecond, they collectively flood the servers when they come back up, "
          "which might knock them down again. We wait, and we wait longer each time. "
          "After 30 seconds we stop growing the wait time, because waiting hours "
          "between retries is worse than occasionally being a little aggressive.", "body"),
        P("Why we add randomness to the wait", "h2"),
        P("Without randomness, all clients that started together are on the same "
          "schedule: they all wait 1 second together, then all try together, then all "
          "fail together. Adding a small random offset spreads them out.", "body"),
        P("Why Reconnected must be first in the queue", "h2"),
        P("When we reconnect, we send the Reconnected signal before any messages from "
          "the new connection. If we sent messages first, the manager would try to "
          "apply them to a book that was synced on a different session. "
          "The queue guarantees ordering, which is the guarantee we rely on.", "body"),
        SP(6), fig_channel_arch(), SP(4),
        CAP("Figure 6 - One writer, one reader, one queue. The RwLock on the book allows many API readers at once."),
        PageBreak(),
    ]

    # ── Part 6: Binance Parser ─────────────────────────────────────────────────
    story += [
        P("Part 6 - ws/binance.rs: Parsing What Binance Actually Sends", "h1"), HR(),
        P("This is the only file that knows about Binance's JSON format. "
          "Everything outside this file works with our own types and never sees "
          "field names like 'b', 'u', or 'U'.", "body"),
        SP(6), fig_json_anatomy(), SP(4),
        CAP("Figure 7 - Every field annotated. We only use the ones relevant to book maintenance."),
        P("Why prices are strings in the JSON", "h2"),
        P("Binance sends prices as text like \"29500.00000000\" rather than numbers. "
          "This is intentional: it lets them control the exact precision without "
          "relying on whatever floating point format the client uses. "
          "We parse them directly into rust_decimal which preserves that precision.", "body"),
        P("How Binance signals a deletion", "h2"),
        P("When a price level needs to be removed from the book, Binance sends the "
          "same update format but with a quantity of zero. That zero is the signal. "
          "Our is_removal() method checks for it. If we missed this we would fill "
          "the book with ghost entries that do not exist on the exchange.", "body"),
        SP(6), fig_deletion(), SP(4),
        CAP("Figure 8 - Before and after a zero-quantity update. The price level at 29,499 is deleted."),
        P("The two-layer design", "h2"),
        P("We define two separate sets of types. RawDepthUpdate matches Binance's JSON "
          "exactly and is private to this file. DepthUpdate is our type with proper "
          "Decimal fields and clear names. The conversion between them is one function. "
          "If Binance renames a field, we change one line.", "body"),
        SP(6), fig_two_layer(), SP(4),
        CAP("Figure 9 - Wire format stays private. Domain type is what the rest of the system uses."),
        P("Sequence numbers", "h2"),
        P("Every update has a first sequence ID (U) and a last sequence ID (u). "
          "These are like page numbers in a book. The manager uses them to detect "
          "whether any updates were missed. This file's job is to carry them faithfully.", "body"),
        SP(6), fig_sequences(), SP(4),
        CAP("Figure 10 - Continuous sequence vs gap. One missing number means the book must be rebuilt."),
        PageBreak(),
    ]

    # ── Part 7: Order Book ─────────────────────────────────────────────────────
    story += [
        P("Part 7 - orderbook/book.rs: The In-Memory Order Book", "h1"), HR(),
        P("This is the heart of the system. Everything else exists to feed data into "
          "this structure or to read from it. It holds the current state of the market: "
          "every price where someone is willing to buy or sell and how much.", "body"),
        SP(6), fig_order_book(), SP(4),
        CAP("Figure 11 - A live order book. Bids on the left sorted highest first. Asks on the right sorted lowest first."),
        P("Why BTreeMap and not HashMap", "h2"),
        P("We need to do two things constantly: update a specific price level by its "
          "exact price, and find the best bid and best ask instantly. "
          "A HashMap gives fast lookup but stores entries in random order, so finding "
          "the best bid requires scanning everything. "
          "A BTreeMap stays sorted automatically with every insert and remove, "
          "so the best bid is always the last entry and the best ask is always the first.", "body"),
        SP(6), fig_btree_vs_hash(), SP(4),
        CAP("Figure 12 - HashMap forces a full scan to find best price. BTreeMap puts it at the end, always."),
        P("What the book looks like in memory", "h2"),
        P("The bids side is one BTreeMap sorted ascending by price. The best bid is "
          "the last entry. We find it by calling next_back() which is one operation "
          "regardless of how many entries are in the map. "
          "The asks side is identical. Best ask is the first entry, found with next().", "body"),
        SP(6), fig_book_memory(), SP(4),
        CAP("Figure 13 - Both maps sorted low to high. One call finds best bid (back) or best ask (front)."),
        P("What happens when an update arrives", "h2"),
        P("For each price level in the update we do exactly one of two things. "
          "If quantity is greater than zero we insert or overwrite that price. "
          "If quantity is zero we remove that price. That is the entire logic.", "body"),
        SP(6), fig_apply_flow(), SP(4),
        CAP("Figure 14 - Two paths only. The BTreeMap handles new vs existing entries automatically."),
        P("The spread", "h2"),
        P("The spread is the gap between the lowest seller and the highest buyer. "
          "It represents the cost of trading immediately: if you buy at the best ask "
          "and sell at the best bid right after, you lose exactly the spread. "
          "A tight spread means a healthy liquid market.", "body"),
        SP(6), fig_spread(), SP(4),
        CAP("Figure 15 - Spread shown as the gap on a price axis. Tighter spread = more liquid market."),
        PageBreak(),
    ]

    # ── Part 8: Manager ────────────────────────────────────────────────────────
    story += [
        P("Part 8 - orderbook/manager.rs: The Sync State Machine", "h1"), HR(),
        P("The manager is the most important piece. It sits between the WebSocket "
          "client and the order book and answers one question at all times: "
          "is it safe to apply this update or not? Getting this wrong means the book "
          "silently diverges from reality and nobody knows until trades go wrong.", "body"),
        SP(6), fig_state_machine(), SP(4),
        CAP("Figure 16 - Three states. Any failure from any state resets to Fetching."),
        P("The three states", "h2"),
        P("Fetching: we are asking Binance's REST API for a full snapshot. "
          "Events from the WebSocket are arriving and queuing up. We do not touch the book.", "body"),
        P("Handshaking: the snapshot arrived. We scan queued events to find the first "
          "one that picks up exactly where the snapshot left off.", "body"),
        P("Live: we found our starting point. Every update gets sequence-checked then applied.", "body"),
        P("Why the snapshot and stream overlap in time", "h2"),
        P("You cannot pause Binance's WebSocket while you fetch the snapshot. "
          "It keeps sending no matter what. By the time the snapshot arrives, some "
          "events in the queue describe changes already in the snapshot. "
          "Others happened after. The handshake sorts them out using sequence numbers.", "body"),
        SP(6), fig_sync_timeline(), SP(4),
        CAP("Figure 17 - REST request takes ~200ms. WebSocket events keep arriving the entire time."),
        SP(6), fig_event_queue(), SP(4),
        CAP("Figure 18 - The queue when snapshot arrives. Stale events are discarded. The first valid one is the starting point."),
        P("The exact handshake rule", "h2"),
        P("Each event has a first ID (U) and a last ID (u). The snapshot has lastUpdateId. "
          "Discard any event where u is less than or equal to lastUpdateId: those changes "
          "are already in the snapshot. Reject and resync if U is greater than "
          "lastUpdateId plus one: there is a gap before the first event. "
          "Apply the event if it passes both checks.", "body"),
        SP(6), fig_handshake(), SP(4),
        CAP("Figure 19 - Every event during handshake passes two checks. Only one outcome leads to applying it."),
        P("Live mode sequence checking", "h2"),
        P("Once live, the check is simpler. Every event's first ID must equal the "
          "previous event's last ID plus one. If the numbers do not connect, "
          "we missed something. Clear the book and start over.", "body"),
        SP(6), fig_continuity(), SP(4),
        CAP("Figure 20 - Good sequence vs gap. One broken link in the chain means the whole book is wrong."),
        P("Why std::sync::RwLock instead of tokio's version", "h2"),
        P("Applying an update to the book is pure computation. No network calls. "
          "No waiting for anything. It takes microseconds. "
          "We use the standard library RwLock because it is simpler and faster for "
          "pure computation. The rule we never break: take the lock, do the work, "
          "release it. Never wait for the network while holding it.", "body"),
        SP(6), fig_rwlock(), SP(4),
        CAP("Figure 21 - Many readers run simultaneously. The writer waits for readers to finish then writes alone."),
        SP(6), tradeoff_table([
            ["Lock type",        "std::sync::RwLock",    "tokio::sync::RwLock", "Book operations are pure computation. std is simpler and faster when you never await inside."],
            ["Snapshot buffering","mpsc internal queue",  "explicit Vec buffer", "While we await the REST call, Tokio suspends our task. The channel queues events automatically."],
            ["Reconnect recovery","restart the run() loop","separate signal task","One loop restart keeps all state transitions in one place and eliminates race conditions."],
        ]),
        PageBreak(),
    ]

    # ── Part 9: HTTP API ───────────────────────────────────────────────────────
    story += [
        P("Part 9 - api/routes.rs: The HTTP API", "h1"), HR(),
        P("The API is how the outside world reads the order book. "
          "Every endpoint takes a read lock on the book, reads what it needs, "
          "releases the lock immediately, then serializes to JSON. "
          "The lock is held for microseconds. Many requests can run at the same time.", "body"),
        SP(6), fig_api_endpoints(), SP(4),
        CAP("Figure 22 - Six endpoints. All GET, all read-only, all return JSON."),
        P("The health endpoint", "h2"),
        P("GET /health returns 200 OK when the book has been synced at least once, "
          "and 503 Service Unavailable while we are still waiting for the first snapshot. "
          "Load balancers and monitoring systems use this to decide whether the instance "
          "is ready to receive traffic. A 503 means: do not send requests yet.", "body"),
        P("Depth limits", "h2"),
        P("The snapshot and bids/asks endpoints accept a depth query parameter. "
          "The default is 20 levels. The maximum is capped at 100. "
          "Without the cap, a caller could request the entire book, which could have "
          "thousands of levels, and cause a slow response that blocks a read lock "
          "far longer than the microseconds we want.", "body"),
        P("The AppState pattern in Axum", "h2"),
        P("Axum passes shared state to every handler by cloning it on each request. "
          "Our AppState holds an Arc, which is a reference-counted pointer. "
          "Cloning an Arc does not copy the book, it just increments a counter. "
          "So every handler gets its own handle to the same book cheaply.", "body"),
        SP(6), fig_http_flow(), SP(4),
        CAP("Figure 23 - A request travels through Axum, hits the handler, acquires a read lock, reads the book, releases, returns JSON."),
        SP(6), tradeoff_table([
            ["Response format",  "JSON via axum::Json",  "Custom binary format", "JSON is readable and debuggable. Binary is faster but adds complexity before we know we need it."],
            ["Depth cap",        "min(requested, 100)",  "No cap",               "Without a cap a single request can lock the book for milliseconds instead of microseconds."],
            ["State sharing",    "Arc<RwLock<>> clone",  "Global singleton",     "Arc makes ownership explicit and testable. Globals are invisible dependencies."],
        ]),
        PageBreak(),
    ]

    # ── Part 10: Config ────────────────────────────────────────────────────────
    story += [
        P("Part 10 - config.rs: Runtime Configuration", "h1"), HR(),
        P("Configuration comes from environment variables. This is the standard approach "
          "for containerized services because it lets you change behavior without "
          "rebuilding the binary and without committing credentials to source control.", "body"),
        SP(6), fig_config(), SP(4),
        CAP("Figure 24 - Environment variables flow into a typed Config struct at startup."),
        P("Why environment variables and not a config file", "h2"),
        P("Config files get committed to source control, which means secrets end up "
          "in git history. They also require the file to be present on the machine, "
          "which means deployment has to place it correctly. "
          "Environment variables are set by the deployment system and never touch disk. "
          "In Docker you set them with -e or in docker-compose.yml.", "body"),
        P("Sensible defaults", "h2"),
        P("Every setting has a default that makes the system work out of the box. "
          "You can run it with zero configuration and it tracks BTCUSDT on port 3000. "
          "You only set environment variables when you want to change something.", "body"),
        P("The channel buffer size", "h2"),
        P("CHANNEL_BUFFER controls how many WebSocket events can queue up while the "
          "manager is busy fetching a REST snapshot. The default is 10,000. "
          "At Binance's update rate of roughly 10 to 100 events per second, "
          "that is enough for a snapshot fetch that takes up to 1,000 seconds, "
          "far more than any reasonable fetch. If this fills up the WebSocket "
          "client task slows down, which is the correct backpressure response.", "body"),
        PageBreak(),
    ]

    # ── Part 11: Wiring ────────────────────────────────────────────────────────
    story += [
        P("Part 11 - main.rs: Wiring Everything Together", "h1"), HR(),
        P("main.rs is short by design. Its only job is to create each component, "
          "connect them to each other, and start the Axum server. "
          "All the real logic lives in the other modules.", "body"),
        SP(6), fig_full_wiring(), SP(4),
        CAP("Figure 25 - main.rs creates the book, channel, and three tasks. Then serves HTTP."),
        P("The startup sequence", "h2"),
        P("First we read configuration from environment variables. "
          "Then we create the order book wrapped in an Arc so it can be shared. "
          "Then we create the mpsc channel, spawn the WebSocket client with the sender end, "
          "spawn the manager with the receiver end and a clone of the book Arc, "
          "give the API layer another clone of the book Arc, "
          "and finally bind and serve the HTTP server. "
          "The server runs in the foreground. When it exits the process ends.", "body"),
        P("Why the HTTP server runs in the foreground", "h2"),
        P("We call tokio::spawn for the WebSocket client and manager because those "
          "are background tasks. We do not spawn the HTTP server because we want "
          "the process to stay alive as long as the server is running. "
          "If we spawned it, main() would return immediately and kill everything.", "body"),
        PageBreak(),
    ]

    # ── Part 12: Dockerfile ────────────────────────────────────────────────────
    story += [
        P("Part 12 - Dockerfile: Shipping the System", "h1"), HR(),
        P("The Dockerfile uses two stages. The first stage compiles the binary using "
          "the full Rust toolchain. The second stage takes only the compiled binary "
          "and puts it in a minimal image with no compiler, no shell, and no extra tools.", "body"),
        SP(6), fig_dockerfile(), SP(4),
        CAP("Figure 26 - Builder image is 1.5GB. Runtime image is ~20MB. Only the binary crosses stages."),
        P("Why two stages", "h2"),
        P("The Rust compiler is enormous. If we used a single stage the production "
          "image would include the entire compiler, all build caches, and all source "
          "code. That is unnecessary weight and a larger attack surface. "
          "Multi-stage builds let us compile in one image and deploy from another.", "body"),
        P("Why we copy Cargo.toml first and compile deps separately", "h2"),
        P("Docker builds layer by layer and caches each layer. If we copied all source "
          "files at once, any change to src/ would invalidate the dependency compilation "
          "layer and force all 200 dependencies to recompile from scratch. "
          "By copying Cargo.toml first and compiling a stub binary, the dependency "
          "compilation gets cached. Only the application code recompiles when src/ changes.", "body"),
        P("Why distroless as the runtime image", "h2"),
        P("Distroless images contain only the minimum runtime libraries. "
          "No shell means no interactive access to a compromised container. "
          "No package manager means no way to install tools after the fact. "
          "The attack surface is as small as it can be.", "body"),
        tradeoff_table([
            ["Build strategy",  "Multi-stage Docker",   "Single stage",    "Single stage ships the full compiler. Multi-stage ships only the binary."],
            ["Runtime image",   "gcr.io/distroless",    "ubuntu or alpine","Distroless has no shell and no package manager. Smaller and more secure."],
            ["Dep caching",     "Cargo.toml copied first","All at once",   "Copying all source at once invalidates the cache on every src change."],
        ]),
        PageBreak(),
    ]

    # ── Part 13: What Comes Next ───────────────────────────────────────────────
    story += [
        P("Part 13 - Production Roadmap: What a Real Trading System Needs", "h1"), HR(),
        P("What we have built is a solid, correct foundation. A production trading "
          "system running at scale needs everything below. Each one is a significant "
          "engineering investment and each one solves a real problem that will "
          "surface when the system is under real load.", "body"),
        SP(8), fig_roadmap(), SP(4),
        CAP("Figure 27 - Twelve production capabilities. Each solves a specific failure mode at scale."),

        P("WebSocket Server API", "h2"),
        P("Right now clients poll our REST API for updates. In a real trading system "
          "every millisecond matters and polling is too slow. "
          "A server-side WebSocket lets clients subscribe to live book updates "
          "and receive them the moment they happen. "
          "Axum supports this with axum::extract::ws. "
          "The challenge is fan-out: one book update needs to go to all subscribers "
          "simultaneously, which requires a tokio::sync::broadcast channel and "
          "careful handling of slow subscribers that fall behind.", "body"),

        P("Prometheus Metrics", "h2"),
        P("You cannot improve what you cannot measure. "
          "A production order book needs counters for events processed per second, "
          "gauges for current book depth and spread, histograms for how long each "
          "update takes to apply, and alerts when sequence gaps occur. "
          "The metrics crate plus prometheus exports give you this. "
          "The principle: measure first, then optimize. Never guess.", "body"),

        P("Rate Limiter on REST Calls", "h2"),
        P("Binance bans clients that make too many REST requests in a short window. "
          "In our reconnect loop the snapshot fetch could fire rapidly during a crash "
          "loop and get the IP banned, making recovery impossible. "
          "A token bucket rate limiter allows a burst of requests then enforces "
          "a steady-state limit. The bucket refills at a fixed rate. "
          "Each request costs one token. If the bucket is empty you wait.", "body"),

        P("Circuit Breaker", "h2"),
        P("A circuit breaker stops retrying after a certain number of consecutive "
          "failures and requires a manual reset or a timeout before trying again. "
          "Without one, a persistent failure like a misconfigured URL generates "
          "an infinite log-spamming retry loop that burns resources and obscures "
          "the root cause. The circuit breaker surfaces the problem clearly "
          "and stops the waste.", "body"),

        P("TimescaleDB for Tick Data", "h2"),
        P("Everything in our system right now exists only in memory. "
          "If the process restarts, all history is gone. "
          "A time-series database like TimescaleDB, which is PostgreSQL with "
          "time-series compression built in, lets you store every depth update "
          "with a nanosecond timestamp. Then you can query: what was the spread "
          "one hour ago? What was the average bid depth yesterday? "
          "These queries power risk engines, backtesting, and audit trails.", "body"),

        P("VWAP Engine", "h2"),
        P("Volume-weighted average price is the average price of all trades weighted "
          "by how much was traded at each price. It is the single most important "
          "metric for evaluating whether a trade executed at a good price. "
          "A VWAP engine runs over a sliding time window and updates continuously "
          "as new trades arrive. It requires a separate trade stream from Binance "
          "and a rolling accumulator that is efficient at the microsecond level.", "body"),

        P("Risk Engine", "h2"),
        P("A risk engine sits above the order book and enforces position limits, "
          "margin requirements, and liquidation triggers. "
          "It reads the best bid and ask continuously and compares them against "
          "open positions. If a position would be underwater at the current price "
          "it triggers a liquidation. "
          "This is where the correctness of our order book becomes critical: "
          "a single wrong price level could trigger an incorrect liquidation "
          "or fail to trigger a necessary one.", "body"),

        P("Multi-Venue Support", "h2"),
        P("Right now we speak only Binance's protocol. "
          "A real trading platform aggregates data from multiple exchanges to get "
          "the best available price across all venues. "
          "This requires abstracting the exchange layer behind a trait: "
          "each exchange implements connect(), parse_update(), and fetch_snapshot(). "
          "The manager, book, and API layer never change regardless of which exchange "
          "is underneath.", "body"),

        P("Graceful Shutdown", "h2"),
        P("When the process receives SIGTERM from the operating system during a "
          "deployment, it should finish processing any events currently in flight "
          "before exiting. Without this, a deployment mid-update could leave the "
          "book in a partially applied state. "
          "Tokio provides CancellationToken for this. "
          "You broadcast a shutdown signal, tasks drain their queues, "
          "and the process exits cleanly.", "body"),

        P("Event Sourcing", "h2"),
        P("Instead of storing only the current state of the book, store every "
          "single depth update as an immutable event with a timestamp. "
          "The current book state is then derived by replaying events from the beginning. "
          "This lets you reconstruct the exact state of the book at any point in time, "
          "audit exactly what happened and when, and replay history to test new logic "
          "against real market data without going live.", "body"),

        P("Load Testing", "h2"),
        P("Before putting this in front of real money, you need to know exactly "
          "where it breaks. A load test simulates 10,000 events per second hitting "
          "the manager while 1,000 concurrent API requests hit the HTTP layer. "
          "You measure where latency climbs, where the CPU saturates, and whether "
          "the RwLock becomes a bottleneck under concurrent read load. "
          "The tool for this in Rust is criterion for microbenchmarks and "
          "a custom harness with tokio for system-level load.", "body"),

        SP(8), tradeoff_table([
            ["WebSocket API",  "broadcast channel fan-out", "polling",         "Polling adds latency. Broadcast delivers the update the instant it happens."],
            ["Metrics",        "Prometheus + Grafana",      "logs only",       "Logs tell you what happened. Metrics tell you what is happening right now."],
            ["Tick storage",   "TimescaleDB",               "PostgreSQL only", "TimescaleDB compresses time-series data 10x better and makes time-range queries fast."],
            ["Multi-venue",    "exchange trait",            "hard-coded URLs", "A trait means adding Coinbase is a new file, not a rewrite of the manager."],
        ]),
    ]

    return story


# ==============================================================================
#  BUILD
# ==============================================================================
def main():
    os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
    doc = SimpleDocTemplate(
        OUT_PATH, pagesize=A4,
        leftMargin=22*mm, rightMargin=22*mm,
        topMargin=22*mm, bottomMargin=24*mm,
        title="Crypto Order Book Engine - System Design",
        author="Mattbusel",
    )
    doc.build(build(), onFirstPage=on_page, onLaterPages=on_page)
    print(f"PDF written: {OUT_PATH}")

if __name__ == "__main__":
    main()
