"""Rendu 3D de l'icône Forge Bar (Blender en ligne de commande).

    blender -b -P assets/icon/render_icon.py -- <dossier de sortie>

Produit icon_512.png (fixe) et frames/NN.png (balancement de 24 images, boucle parfaite), fond transparent.
`assets/icon/build_icon.py` en tire forge-bar.ico et l'animation WebP.
"""
import math
import os
import sys

import bpy

out = os.path.abspath(sys.argv[sys.argv.index("--") + 1] if "--" in sys.argv else "out")
os.makedirs(os.path.join(out, "frames"), exist_ok=True)

bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.device = "CPU"
scene.cycles.samples = 56
scene.cycles.use_denoising = True
scene.render.film_transparent = True
scene.render.image_settings.file_format = "PNG"
scene.render.image_settings.color_mode = "RGBA"
scene.view_settings.view_transform = "Standard"


def mat(name, color, rough=0.3, metal=0.0, emit=None, strength=0.0, coat=0.0, alpha=1.0):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = color
    b.inputs["Roughness"].default_value = rough
    b.inputs["Metallic"].default_value = metal
    if "Coat Weight" in b.inputs:
        b.inputs["Coat Weight"].default_value = coat
    if emit is not None:
        b.inputs["Emission Color"].default_value = emit
        b.inputs["Emission Strength"].default_value = strength
    return m


def rounded_box(name, size, radius, location, material):
    bpy.ops.mesh.primitive_cube_add(size=1, location=location)
    o = bpy.context.active_object
    o.name = name
    o.scale = size
    bpy.ops.object.transform_apply(scale=True)
    bev = o.modifiers.new("bevel", "BEVEL")
    bev.width = radius
    bev.segments = 10
    bev.limit_method = "NONE"
    bpy.ops.object.shade_smooth()
    o.data.materials.append(material)
    return o


root = bpy.data.objects.new("root", None)
scene.collection.objects.link(root)

# Socle : tuile arrondie bleu nuit, vernis brillant.
tile = rounded_box("tile", (2.0, 2.0, 0.34), 0.34, (0, 0, 0), mat("tile", (0.03, 0.05, 0.16, 1), 0.22, 0.2, coat=1.0))
# Barre de verre fumé en relief.
bar = rounded_box("bar", (1.66, 0.62, 0.2), 0.14, (0, 0.02, 0.26), mat("bar", (0.015, 0.02, 0.04, 1), 0.08, 0.5, coat=1.0))
# Filet lumineux sur le bord supérieur de la barre.
rim = rounded_box("rim", (1.5, 0.045, 0.05), 0.02, (0, 0.3, 0.37), mat("rim", (0.2, 0.7, 1, 1), 0.2, emit=(0.25, 0.75, 1, 1), strength=2.5))
# Trois widgets lumineux : cyan, violet, ambre (la braise de la forge).
orbs = []
for x, col in ((-0.5, (0.2, 0.8, 1.0)), (0.0, (0.62, 0.42, 1.0)), (0.5, (1.0, 0.62, 0.12))):
    bpy.ops.mesh.primitive_uv_sphere_add(radius=0.15, location=(x, 0.02, 0.43), segments=48, ring_count=24)
    s = bpy.context.active_object
    bpy.ops.object.shade_smooth()
    s.data.materials.append(mat("orb", (*col, 1), 0.15, emit=(*col, 1), strength=2.4))
    orbs.append(s)
# Reflet de dessous : deux petits traits, comme une barre de progression.
for i, w in enumerate((1.1, 0.7)):
    rounded_box("line%d" % i, (w, 0.07, 0.04), 0.02, (-(1.1 - w) / 2 - 0.1 if i else -0.0, -0.58 - i * 0.2, 0.2),
                mat("line", (0.55, 0.7, 1, 1), 0.3, emit=(0.4, 0.6, 1, 1), strength=0.7))

for o in (tile, bar, rim, *orbs):
    o.parent = root
for o in bpy.data.objects:
    if o.name.startswith("line"):
        o.parent = root

# Lumières : grande source douce en haut à gauche, contre-jour bleu, remplissage chaud.
def area(loc, energy, color, size):
    bpy.ops.object.light_add(type="AREA", location=loc)
    a = bpy.context.active_object
    a.data.energy = energy
    a.data.color = color
    a.data.size = size
    d = (-a.location).to_track_quat("-Z", "Y")
    a.rotation_euler = d.to_euler()


area((-3, -4, 5), 420, (1, 1, 1), 4)
area((4, 2, 3), 220, (0.35, 0.55, 1), 3)
area((0, -5, 1), 120, (1, 0.8, 0.6), 4)
world = bpy.data.worlds.new("w")
scene.world = world
world.use_nodes = True
bg = world.node_tree.nodes["Background"]
bg.inputs["Color"].default_value = (0.05, 0.07, 0.15, 1)
bg.inputs["Strength"].default_value = 0.6

bpy.ops.object.camera_add(location=(0, -2.0, 6.6))
cam = bpy.context.active_object
cam.data.lens = 100
cam.rotation_euler = (math.radians(16.9), 0, 0)
scene.camera = cam


def render(path, size, yaw_deg, pitch_deg=10):
    # Rotation autour de l'axe vertical de l'image (Y) : la tuile pivote comme une porte, en vraie 3D.
    root.rotation_euler = (math.radians(pitch_deg), math.radians(yaw_deg), 0)
    scene.render.resolution_x = scene.render.resolution_y = size
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)


render(os.path.join(out, "icon_512.png"), 512, -20)
FRAMES = 24
for i in range(FRAMES):
    # Balancement doux gauche-droite, boucle parfaite.
    render(os.path.join(out, "frames", "%02d.png" % i), 192, 22 * math.sin(2 * math.pi * i / FRAMES))
print("rendu terminé", out)
