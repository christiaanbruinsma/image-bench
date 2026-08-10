conf = configuration_data()
conf.set('APP_ID', application_id)

configure_file(
  input: base_id + '.desktop.in',
  output: application_id + '.desktop',
  configuration: conf,
  install: true,
  install_dir: datadir / 'applications',
)

configure_file(
  input: base_id + '.metainfo.xml.in',
  output: application_id + '.metainfo.xml',
  configuration: conf,
  install: true,
  install_dir: datadir / 'metainfo',
)

install_data(
  'icons/' + base_id + '.svg',
  install_dir: datadir / 'icons/hicolor/scalable/apps',
  rename: application_id + '.svg',
)

install_data(
  'icons/' + base_id + '-symbolic.svg',
  install_dir: datadir / 'icons/hicolor/symbolic/apps',
  rename: application_id + '-symbolic.svg',
)

install_data(
  'icons/hicolor/scalable/actions/panel-left-symbolic.svg',
  'icons/hicolor/scalable/actions/panel-right-symbolic.svg',
  install_dir: datadir / 'icons/hicolor/scalable/actions',
)

