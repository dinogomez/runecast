# dmgbuild settings for the drag-to-Applications window, read by package-macos.sh from dist/.
# The icon centres sit on the two slots drawn in assets/dmg/background.png (640 x 400 points;
# dmgbuild pairs it with background@2x.png into one Retina image).
format = "UDZO"
files = ["Runecast.app"]
symlinks = {"Applications": "/Applications"}
background = "../assets/dmg/background.png"
window_rect = ((200, 200), (640, 400))
icon_size = 128
text_size = 13
icon_locations = {"Runecast.app": (170, 192), "Applications": (470, 192)}
hide_extensions = ["Runecast.app"]
show_status_bar = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
