from PIL import Image, ImageDraw, ImageFont
import os, math

COSM = '/home/dusty/arty/deploy/assets/cosmetics'
OUT  = '/tmp/hat_galleries'
os.makedirs(OUT, exist_ok=True)

BG       = (30, 30, 35, 255)
CELL_BG  = (40, 40, 48, 255)
GOLD     = (255, 200, 50)
WHITE    = (240, 240, 240)
WB_CLR   = (120, 160, 255)

try:
    fn = ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf', 18)
    fs = ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf', 15)
    ft = ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf', 22)
except:
    fn = fs = ft = ImageFont.load_default()

def make_gallery(items, filename, title, prefix, base_w, base_h, scale, cols=6):
    """items: list of (id, name, cost, currency)"""
    PAD     = 14
    LABEL_H = 36
    TITLE_H = 54
    w = int(base_w * scale)
    h = int(base_h * scale)
    cell_w = w + PAD * 2
    cell_h = h + PAD * 2 + LABEL_H
    rows = math.ceil(len(items) / cols)
    img_w = cols * cell_w + PAD
    img_h = TITLE_H + rows * cell_h + PAD

    img = Image.new('RGBA', (img_w, img_h), BG)
    draw = ImageDraw.Draw(img)
    draw.rectangle([0, 0, img_w, TITLE_H], fill=(20, 20, 25, 255))
    draw.text((PAD, 14), title, fill=GOLD, font=ft)

    for i, (iid, name, cost, cur) in enumerate(items):
        col = i % cols
        row = i // cols
        cx = PAD + col * cell_w
        cy = TITLE_H + PAD//2 + row * cell_h
        draw.rectangle([cx, cy, cx+cell_w-PAD//2, cy+cell_h-PAD//2],
                       fill=CELL_BG, outline=(60,60,70), width=1)
        path = os.path.join(COSM, f'{prefix}{iid}.png')
        if os.path.exists(path):
            sprite = Image.open(path).convert('RGBA')
            sprite = sprite.resize((w, h), Image.NEAREST)
            cb = Image.new('RGBA', (w, h), (50, 50, 58, 255))
            img.paste(cb, (cx+PAD//2, cy+PAD//2))
            img.paste(sprite, (cx+PAD//2, cy+PAD//2), sprite)
        ly = cy + h + PAD//2 + 2
        draw.text((cx+PAD//2, ly), name, fill=WHITE, font=fs)
        cc = WB_CLR if cur == 'WB' else GOLD
        draw.text((cx+PAD//2, ly+17), f'{cost} {cur}', fill=cc, font=fs)

    out = os.path.join(OUT, filename)
    img.convert('RGB').save(out, 'PNG')
    print(f'  {out}  ({img_w}x{img_h})')
    return out

# ── HATS ─────────────────────────────────────────────────────────────────────
hats_scrap = [
    (1,"Top Hat",200,'SC'),(2,"Propeller Hat",350,'SC'),(3,"Flower",150,'SC'),
    (4,"Crown",400,'SC'),(5,"Fez",250,'SC'),(6,"Beret",200,'SC'),
    (7,"Party Hat",200,'SC'),(8,"Halo",500,'SC'),(9,"Devil Horns",500,'SC'),
    (12,"Blue Party Hat",200,'SC'),(13,"Cowboy Hat",350,'SC'),(14,"Pirate Hat",500,'SC'),
    (15,"Viking Helm",550,'SC'),(16,"Beanie",150,'SC'),(17,"Bandana",150,'SC'),
    (18,"Angel Ring",500,'SC'),(19,"Horn Nubs",450,'SC'),(20,"Laurel Wreath",350,'SC'),
    (21,"Party Hat 2",200,'SC'),(22,"Pirate Tricorn",500,'SC'),(23,"Mohawk",300,'SC'),
    (24,"Bow",200,'SC'),(25,"Frontier Hat",350,'SC'),(26,"War Helm",500,'SC'),
    (27,"Sombrero",300,'SC'),(28,"Luchador Mask",600,'SC'),(29,"Mortarboard",300,'SC'),
    (30,"Baseball Cap",200,'SC'),(31,"Samurai Helm",550,'SC'),(32,"Obsidian Crown",1500,'SC'),
    (33,"Pharaoh Headdress",1800,'SC'),(34,"Demon King Horns",1600,'SC'),
    (35,"Astronaut Helmet",1500,'SC'),(36,"Dragon Skull",2000,'SC'),
]
hats_wb = [
    (10,"Gold Crown",50,'WB'),(11,"Laurel Wreath",30,'WB'),
    (38,"Cosmic Crown",150,'WB'),(39,"Phoenix Crest",120,'WB'),
    (40,"Void Wraith Hood*",200,'WB'),(41,"Gilded Jester",100,'WB'),
    (42,"Crimson War Mask*",200,'WB'),
]

# ── GUNS ─────────────────────────────────────────────────────────────────────
guns = [
    (1,"Pistol",200,'SC'),(2,"Shotgun",300,'SC'),(3,"Sniper",400,'SC'),
    (4,"Minigun",500,'SC'),(5,"Cannon",500,'SC'),(6,"Laser",30,'WB'),
    (7,"Golden Gun",40,'WB'),(8,"Revolver",350,'SC'),(9,"Flamethrower",650,'SC'),
    (10,"Rocket Launcher",800,'SC'),(11,"SMG",350,'SC'),(12,"Flintlock",500,'SC'),
    (13,"Crossbow",600,'SC'),(14,"Revolver",400,'SC'),(15,"Laser Pistol",500,'SC'),
    (16,"Gold Musket",900,'SC'),(17,"Fusion Rifle",650,'SC'),(18,"Obsidian Cannon",1800,'SC'),
    (19,"Crystal Sniper",1500,'SC'),(20,"Dragon's Breath",2000,'SC'),
    (21,"Blood Revolver",1600,'SC'),(22,"Thunder Rail",1800,'SC'),
]

# ── BOOTS ────────────────────────────────────────────────────────────────────
boots = [
    (1,"Red Boots",100,'SC'),(2,"White Boots",100,'SC'),(3,"Gold Boots",150,'SC'),
    (4,"Combat Green",100,'SC'),(5,"Electric Blue",20,'WB'),
]

# ── UNIFORMS (color swatches — no separate PNGs, use hat_1 as placeholder) ──
# Uniforms don't have separate cosmetic PNGs — they're palette swaps.
# We'll make a simple text-only card grid for them.
uniforms = [
    (1,"Camo Green",200,'SC'),(2,"Desert Tan",200,'SC'),(3,"Midnight Black",300,'SC'),
    (4,"Snow White",300,'SC'),(5,"Navy",250,'SC'),(6,"Pink Camo",30,'WB'),
    (7,"Gold Plate",40,'WB'),
]
UNIFORM_COLORS = {
    1:(80,120,60),2:(180,150,100),3:(30,30,30),
    4:(230,230,230),5:(30,60,100),6:(220,100,150),7:(200,160,30),
}

def make_uniform_gallery(items, filename, title):
    PAD     = 14
    LABEL_H = 36
    TITLE_H = 54
    cols = 6
    sw   = 132
    sh   = 80
    cell_w = sw + PAD * 2
    cell_h = sh + PAD * 2 + LABEL_H
    rows = math.ceil(len(items) / cols)
    img_w = cols * cell_w + PAD
    img_h = TITLE_H + rows * cell_h + PAD
    img = Image.new('RGBA', (img_w, img_h), BG)
    draw = ImageDraw.Draw(img)
    draw.rectangle([0, 0, img_w, TITLE_H], fill=(20, 20, 25, 255))
    draw.text((PAD, 14), title, fill=GOLD, font=ft)
    for i, (iid, name, cost, cur) in enumerate(items):
        col = i % cols
        row = i // cols
        cx = PAD + col * cell_w
        cy = TITLE_H + PAD//2 + row * cell_h
        draw.rectangle([cx, cy, cx+cell_w-PAD//2, cy+cell_h-PAD//2],
                       fill=CELL_BG, outline=(60,60,70), width=1)
        color = UNIFORM_COLORS.get(iid, (100,100,100))
        draw.rectangle([cx+PAD//2, cy+PAD//2, cx+PAD//2+sw, cy+PAD//2+sh],
                       fill=color, outline=(80,80,80), width=1)
        ly = cy + sh + PAD//2 + 2
        draw.text((cx+PAD//2, ly), name, fill=WHITE, font=fs)
        cc = WB_CLR if cur == 'WB' else GOLD
        draw.text((cx+PAD//2, ly+17), f'{cost} {cur}', fill=cc, font=fs)
    out = os.path.join(OUT, filename)
    img.convert('RGB').save(out, 'PNG')
    print(f'  {out}  ({img_w}x{img_h})')
    return out

print('Generating galleries...')
f1 = make_gallery(hats_scrap, 'hats_scrap.png',    '🪖 HATS — SCRAP',    'hat_', 66, 60, 2.5, cols=6)
f2 = make_gallery(hats_wb,    'hats_warbonds.png',  '🌟 HATS — WARBONDS  (* = replaces head)', 'hat_', 66, 60, 2.5, cols=6)
f3 = make_gallery(guns,       'guns.png',            '🔫 GUN STYLES',      'gun_', 138, 78, 1.5, cols=5)
f4 = make_gallery(boots,      'boots.png',           '👟 BOOTS',           'boot_', 12, 9, 8, cols=5)
f5 = make_uniform_gallery(uniforms, 'uniforms.png', '🎽 UNIFORMS')
print('All done.')
