"""Génère le catalogue de thèmes intégrés dans themes/.

Chaque thème est un fichier TOML autonome ; les utilisateurs peuvent en
partir avec `extends = "<id>"` dans leur propre fichier.
"""
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "themes"

# id, nom, famille, backdrop, fond (couleurs), type, opacité, texte, secondaire, accent, rayon, police, extra
T = []
def t(id, name, family, bg, text, sec, accent, kind="solid", opacity=1.0, backdrop="none",
      radius=0.0, font="Segoe UI Variable Text", angle=0.0, border="", size=12.0, spacing=14.0):
    T.append(dict(id=id, name=name, family=family, bg=bg, text=text, sec=sec, accent=accent, kind=kind,
                  opacity=opacity, backdrop=backdrop, radius=radius, font=font, angle=angle,
                  border=border, size=size, spacing=spacing))

# Windows natif (8)
t("fluent-dark", "Fluent sombre", "windows", ["#202020"], "#FFFFFF", "#C5C5C5", "#4CC2FF")
t("fluent-light", "Fluent clair", "windows", ["#F3F3F3"], "#1B1B1B", "#5C5C5C", "#005FB8")
t("mica-dark", "Mica sombre", "windows", ["#202020"], "#FFFFFF", "#C5C5C5", "#4CC2FF", opacity=0.35, backdrop="mica")
t("mica-light", "Mica clair", "windows", ["#F3F3F3"], "#1B1B1B", "#5C5C5C", "#005FB8", opacity=0.35, backdrop="mica")
t("acrylic-dark", "Acrylic sombre", "windows", ["#2C2C2C"], "#FFFFFF", "#C5C5C5", "#4CC2FF", opacity=0.45, backdrop="acrylic")
t("acrylic-light", "Acrylic clair", "windows", ["#FCFCFC"], "#1B1B1B", "#5C5C5C", "#005FB8", opacity=0.45, backdrop="acrylic")
t("high-contrast", "Contraste élevé", "windows", ["#000000"], "#FFFFFF", "#FFFF00", "#00FFFF", border="#FFFFFF")
t("windows-10", "Windows 10", "windows", ["#1F1F1F"], "#FFFFFF", "#BFBFBF", "#0078D7", font="Segoe UI")

# Dégradés (8)
g = lambda id, name, cols, text="#FFFFFF", sec="#F0F0F0", accent="#FFFFFF", angle=0.0: t(
    id, name, "gradient", cols, text, sec, accent, kind="linear", angle=angle)
g("aurora", "Aurora", ["#0F2027", "#203A43", "#2C5364"], accent="#7FFFD4")
g("sunset", "Sunset", ["#FF5F6D", "#FFC371"], accent="#2B1B17", text="#2B1B17", sec="#4A2E26")
g("ocean", "Ocean", ["#1A2980", "#26D0CE"], accent="#E0FFFF")
g("forest", "Forêt", ["#134E5E", "#71B280"], accent="#E8F5C8")
g("lavender", "Lavande", ["#8E78C8", "#C3A6E8"], accent="#2E1A47", text="#1E1030", sec="#3A2858")
g("ember", "Braise", ["#3A0D06", "#B22A0E", "#F0761D"], accent="#FFD27F")
g("peach", "Pêche", ["#FFB199", "#FF0844"], accent="#FFF3B0", angle=20.0)
g("midnight", "Minuit", ["#000428", "#004E92"], accent="#8AB4FF")

# Néon et gaming (8)
n = lambda id, name, cols, accent, kind="solid", border="": t(
    id, name, "neon", cols, "#F5F5F5", "#A8A8B8", accent, kind=kind, font="Cascadia Mono", border=border, size=11.5)
n("cyberpunk", "Cyberpunk", ["#0D0221", "#2A0944"], "#FF2A6D", kind="linear", border="#05D9E8")
n("synthwave", "Synthwave", ["#241734", "#2E1F47", "#B33F62"], "#F9C80E", kind="linear", border="#FF6AD5")
n("rgb", "RGB", ["#FF0000", "#00FF00", "#0000FF", "#FF00FF"], "#FFFFFF", kind="linear")
n("matrix", "Matrix", ["#000000"], "#00FF41", border="#003B00")
n("plasma", "Plasma", ["#10002B", "#5A189A", "#9D4EDD"], "#E0AAFF", kind="linear")
n("toxic", "Toxique", ["#0B0F0B"], "#B7FF00", border="#B7FF00")
n("ice-neon", "Néon glacé", ["#020B18"], "#00E5FF", border="#00E5FF")
n("crimson", "Cramoisi", ["#120005"], "#FF003C", border="#FF003C")

# Palettes de développeur (8)
d = lambda id, name, bg, text, sec, accent: t(id, name, "dev", [bg], text, sec, accent, font="Cascadia Mono", size=11.5)
d("dracula", "Dracula", "#282A36", "#F8F8F2", "#BD93F9", "#FF79C6")
d("nord", "Nord", "#2E3440", "#ECEFF4", "#D8DEE9", "#88C0D0")
d("gruvbox", "Gruvbox", "#282828", "#EBDBB2", "#BDAE93", "#FABD2F")
d("catppuccin", "Catppuccin Mocha", "#1E1E2E", "#CDD6F4", "#BAC2DE", "#CBA6F7")
d("tokyo-night", "Tokyo Night", "#1A1B26", "#C0CAF5", "#A9B1D6", "#7AA2F7")
d("solarized", "Solarized sombre", "#002B36", "#EEE8D5", "#93A1A1", "#B58900")
d("one-dark", "One Dark", "#282C34", "#ABB2BF", "#828997", "#61AFEF")
d("monokai", "Monokai", "#272822", "#F8F8F2", "#CFCFC2", "#A6E22E")

# Glass (6) : barre flottante, coins arrondis
gl = lambda id, name, bg, text, sec, accent, opacity, backdrop="acrylic": t(
    id, name, "glass", [bg], text, sec, accent, opacity=opacity, backdrop=backdrop, radius=12.0, border="#FFFFFF33")
gl("glass-clear", "Verre clair", "#FFFFFF", "#111111", "#3A3A3A", "#005FB8", 0.25)
gl("glass-smoke", "Verre fumé", "#000000", "#FFFFFF", "#D0D0D0", "#4CC2FF", 0.35)
gl("glass-frost", "Givre", "#DDEEFF", "#0B2540", "#33536F", "#1C7CD6", 0.30)
gl("glass-rose", "Cristal rose", "#FF8FB1", "#2A0A16", "#5A2338", "#B0004A", 0.28)
gl("glass-emerald", "Cristal émeraude", "#2ECC71", "#04210F", "#1E4A30", "#00692F", 0.28)
gl("glass-mica", "Verre Mica", "#202020", "#FFFFFF", "#D0D0D0", "#4CC2FF", 0.20, backdrop="mica")

# Minimal (6)
m = lambda id, name, bg, text, sec, accent, opacity=1.0: t(id, name, "minimal", [bg], text, sec, accent, opacity=opacity, spacing=18.0)
m("mono-dark", "Mono sombre", "#111111", "#EDEDED", "#8A8A8A", "#EDEDED")
m("mono-light", "Mono clair", "#FAFAFA", "#111111", "#777777", "#111111")
m("ink", "Encre", "#0A0A0A", "#F2F2F2", "#6E6E6E", "#C9A227")
m("transparent-dark", "Transparent (texte clair)", "#000000", "#FFFFFF", "#DDDDDD", "#FFFFFF", opacity=0.0)
m("transparent-light", "Transparent (texte sombre)", "#FFFFFF", "#000000", "#333333", "#000000", opacity=0.0)
m("slate", "Ardoise", "#1E293B", "#E2E8F0", "#94A3B8", "#38BDF8")

# Rétro (6)
r = lambda id, name, cols, text, sec, accent, kind="solid", font="Segoe UI", border="": t(
    id, name, "retro", cols, text, sec, accent, kind=kind, font=font, border=border)
r("y2k", "Années 2000", ["#245EDC", "#3A81F3", "#245EDC"], "#FFFFFF", "#DCE8FF", "#7CFC00", kind="linear", font="Tahoma")
r("classic-grey", "Gris classique", ["#C0C0C0"], "#000000", "#404040", "#000080", font="Tahoma", border="#FFFFFF")
r("green-terminal", "Terminal vert", ["#001100"], "#33FF33", "#1E9E1E", "#33FF33", font="Consolas")
r("amber-terminal", "Terminal ambre", ["#120A00"], "#FFB000", "#B37B00", "#FFB000", font="Consolas")
r("paper", "Papier", ["#F4ECD8"], "#3B2F2F", "#6B5B4B", "#A0522D", font="Georgia")
r("pixel", "Pixel", ["#1D2B53"], "#FFF1E8", "#C2C3C7", "#FF004D", font="Consolas")

assert len(T) == 50, len(T)
assert len({x["id"] for x in T}) == 50

def arr(xs):
    return "[" + ", ".join(f'"{x}"' for x in xs) + "]"

OUT.mkdir(exist_ok=True)
for x in T:
    (OUT / f"{x['id']}.toml").write_text(f'''# Thème intégré Forge Bar — copiez ce fichier dans %APPDATA%\\ForgeBar\\themes pour le modifier,
# ou créez un fichier avec `extends = "{x['id']}"` pour n'en changer que quelques valeurs.
name = "{x['name']}"
family = "{x['family']}"
backdrop = "{x['backdrop']}"
radius = {x['radius']:.1f}
border_width = {1.0 if x['border'] else 0.0:.1f}
font_family = "{x['font']}"
mono_family = "Cascadia Mono"
font_size = {x['size']:.1f}
spacing = {x['spacing']:.1f}

[background]
kind = "{x['kind']}"
colors = {arr(x['bg'])}
angle = {x['angle']:.1f}
opacity = {x['opacity']:.2f}

[colors]
text = "{x['text']}"
text_secondary = "{x['sec']}"
accent = "{x['accent']}"
border = "{x['border']}"
''', encoding="utf-8")
print(len(T), "thèmes écrits")
