<script>
  import {
    startRecording,
    stopRecording,
    onMeter,
    startAudioStream,
    stopAudioStream,
    getCapabilities,
    requestPermissions
  } from 'tauri-plugin-microphone-kessdev'

  let status = $state('idle')
  let log = $state('')
  let recording = $state(false)
  let metering = $state(false)
  let streaming = $state(false)
  let meterLevel = $state(0)
  let capabilities = $state('')

  let unlistenMeter = null
  let audioStream = null

  function append(message) {
    log += `[${new Date().toLocaleTimeString()}] ${message}\n`
  }

  async function toggleRecording() {
    try {
      if (!recording) {
        await requestPermissions()
        await startRecording({ format: 'wav' })
        recording = true
        status = 'recording'
        append('recording (wav)…')
      } else {
        const clip = await stopRecording()
        recording = false
        status = 'idle'
        append(`clip: format=${clip.format} duration=${clip.duration.toFixed(2)}s`)
        if (clip.path) append(`path: ${clip.path}`)
        if (clip.url) append(`url: ${clip.url}`)
      }
    } catch (error) {
      append(`error: ${error}`)
    }
  }

  function toggleMeter() {
    if (!metering) {
      unlistenMeter = onMeter((data) => {
        meterLevel = data.level
      })
      metering = true
      append('meter on')
    } else {
      unlistenMeter?.()
      unlistenMeter = null
      metering = false
      meterLevel = 0
      append('meter off')
    }
  }

  async function toggleStream() {
    try {
      if (!streaming) {
        audioStream = await startAudioStream({ frameSamples: 1024 }, (_chunk) => {
          // Consume raw PCM here (waveform / VAD / upload).
        })
        streaming = true
        append(`stream on (transport=${audioStream.info.transport})`)
      } else {
        await audioStream?.stop()
        audioStream = null
        streaming = false
        append('stream off')
      }
    } catch (error) {
      append(`error: ${error}`)
    }
  }

  async function showCapabilities() {
    capabilities = JSON.stringify(await getCapabilities(), null, 2)
  }
</script>

<main class="container">
  <h1>Microphone plugin</h1>

  <div class="row">
    <button onclick={toggleRecording}>
      {recording ? 'Stop recording' : 'Start recording'}
    </button>
    <button onclick={toggleMeter}>{metering ? 'Stop meter' : 'Start meter'}</button>
    <button onclick={toggleStream}>{streaming ? 'Stop stream' : 'Start stream'}</button>
    <button onclick={showCapabilities}>Capabilities</button>
  </div>

  <p>State: <strong>{status}</strong></p>

  <div class="meter">
    <div class="meter-fill" style={`width: ${Math.min(meterLevel, 1) * 100}%`}></div>
  </div>

  {#if capabilities}
    <pre>{capabilities}</pre>
  {/if}

  <pre class="log">{log}</pre>
</main>

<style>
  :root {
    font-family: system-ui, sans-serif;
  }

  .container {
    max-width: 720px;
    margin: 0 auto;
    padding: 1rem;
  }

  .row {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  button {
    padding: 0.5rem 0.9rem;
    border-radius: 6px;
    border: 1px solid #ccc;
    background: #f6f6f6;
    cursor: pointer;
  }

  .meter {
    height: 16px;
    background: #eee;
    border-radius: 8px;
    overflow: hidden;
    margin: 0.75rem 0;
  }

  .meter-fill {
    height: 100%;
    background: linear-gradient(90deg, #35c759, #ffcc00, #ff3b30);
    transition: width 80ms linear;
  }

  .log {
    background: #111;
    color: #7CFC00;
    padding: 0.75rem;
    border-radius: 6px;
    min-height: 8rem;
    white-space: pre-wrap;
    overflow: auto;
  }
</style>
