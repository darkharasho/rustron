// Bench probe: time-to-grid, JS speed, scroll smoothness, typing latency. Posts raw results to the report URL.
(() => {
  const q = new URLSearchParams(location.search)
  const t0 = +q.get('t0'), report = q.get('report')
  const R = { engine: q.get('engine') || 'unknown' }
  const raf = () => new Promise(r => requestAnimationFrame(r)), sleep = ms => new Promise(r => setTimeout(r, ms))
  const send = body => fetch(report, { method: 'POST', mode: 'no-cors', headers: { 'content-type': 'text/plain' }, body: JSON.stringify(body) })
  async function run() {
    for (;;) { await raf(); if (document.querySelectorAll('img').length >= 10) break }
    R.gridSinceLaunch = Date.now() - t0; R.gridSinceNav = Math.round(performance.now())
    await sleep(2000)
    // JS speed
    let a = performance.now(); for (let i = 0; i < 10; i++) JSON.parse(JSON.stringify(window.__FIX)); R.jsonMs = +(performance.now() - a).toFixed(1)
    const fix = window.__FIX || {}
    const games = Object.values(fix).find(v => Array.isArray(v) && v.length > 100) || Object.values(fix).map(v => v && (v.games || v.items)).find(Array.isArray) || []
    a = performance.now(); for (let i = 0; i < 300; i++) [...games].sort((x, y) => String(x.name || x.title).localeCompare(String(y.name || y.title))); R.sortMs = +(performance.now() - a).toFixed(1)
    a = performance.now(); let h = 0; for (let i = 0; i < 2e7; i++) h = (h * 31 + i) | 0; R.loopMs = +(performance.now() - a).toFixed(1)
    // scroll
    const sc = [...document.querySelectorAll('*')].filter(e => e.scrollHeight > e.clientHeight + 50 && /(auto|scroll)/.test(getComputedStyle(e).overflowY)).sort((x, y) => y.scrollHeight - x.scrollHeight)[0]
    const iv = []
    if (sc) {
      const range = sc.scrollHeight - sc.clientHeight
      let last = await raf()
      for (let f = 0; f < 240; f++) { sc.scrollTop = (sc.scrollTop + 12) % Math.max(1, range); const t = await raf(); iv.push(+(t - last).toFixed(2)); last = t }
      sc.scrollTop = 0; await sleep(500)
    }
    R.frameMs = iv
    // typing: input event -> next painted frame
    const inp = document.querySelector('input[type=search],input[placeholder*="earch"],input')
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set, lat = []
    if (inp) {
      inp.focus(); let v = ''
      for (const ch of 'lethal company') { v += ch; await raf(); const s = performance.now(); set.call(inp, v); inp.dispatchEvent(new Event('input', { bubbles: true })); await raf(); await raf(); lat.push(+(performance.now() - s).toFixed(2)); await sleep(60) }
    }
    R.typeMs = lat
    R.errors = (window.__ERR || []).length
    await send(R)
  }
  run().catch(e => send({ gridSinceLaunch: 0, gridSinceNav: 0, jsonMs: 0, sortMs: 0, loopMs: 0, frameMs: [], typeMs: [], errors: 0, ...R, fail: String(e && e.stack || e) }))
})()
