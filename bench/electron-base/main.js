// Electron baseline: loads the same prepared fixture Rustron gets. The probe inside the page reports results.
const { app, BrowserWindow } = require('electron')
// The fixture's module script has a CORS attribute, which Chromium refuses to load from file:// otherwise.
app.commandLine.appendSwitch('allow-file-access-from-files')
app.whenReady().then(async () => {
  const w = new BrowserWindow({ width: 1200, height: 780, frame: false, backgroundColor: '#0f191d', webPreferences: { contextIsolation: true } })
  await w.loadURL(process.env.RUSTRON_BENCH_URL)
})
