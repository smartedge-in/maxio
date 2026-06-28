try {
  const saved = localStorage.getItem('theme')
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches
  const shouldUseDark =
    saved === 'dark' || ((saved === null || saved === 'system') && prefersDark)
  document.documentElement.classList.toggle('dark', shouldUseDark)
} catch (_) {
  document.documentElement.classList.toggle(
    'dark',
    window.matchMedia('(prefers-color-scheme: dark)').matches
  )
}