project(
  'image-bench',
  'rust',
  version: '0.9.1',
  meson_version: '>= 1.4.0',
  license: 'GPL-3.0-or-later',
)

gnome = import('gnome')
i18n = import('i18n')

base_id = 'io.github.christiaanbruinsma.ImageBench'
is_devel = get_option('profile') == 'development'
application_id = is_devel ? base_id + '.Devel' : base_id

bindir = get_option('prefix') / get_option('bindir')
datadir = get_option('prefix') / get_option('datadir')
localedir = get_option('prefix') / get_option('localedir')

subdir('data')
subdir('po')
subdir('src')

gnome.post_install(
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
