import './style.css'

document.querySelector<HTMLDivElement>('#app')?.insertAdjacentHTML(
  'afterbegin',
  `
  <main>
    <h1>rsnap</h1>
    <p>Vite frontend scaffold is ready.</p>
    <button id="action-stub">Placeholder Action</button>
  </main>
`
)

document
  .querySelector<HTMLButtonElement>('#action-stub')
  ?.addEventListener('click', () => {
    console.log('Button stub clicked')
  })
