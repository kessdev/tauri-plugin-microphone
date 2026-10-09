import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

vi.mock('@tauri-apps/api/core', () => {
  class Channel {
    onmessage: ((message: unknown) => void) | null = null
    constructor() {
      ;(globalThis as any).__lastChannel = this
    }
  }
  return {
    invoke: vi.fn(),
    convertFileSrc: (p: string) => `asset://${p}`,
    requestPermissions: vi.fn(),
    checkPermissions: vi.fn(),
    addPluginListener: vi.fn(),
    Channel
  }
})

import {
  invoke,
  requestPermissions,
  checkPermissions,
  addPluginListener
} from '@tauri-apps/api/core'
import {
  isMobile,
  formatFromMimeType,
  resolveOutputFormat,
  startRecording,
  stopRecording,
  pauseRecording,
  resumeRecording,
  getRecordingState,
  getCapabilities,
  onMeter,
  startAudioStream,
  stopAudioStream,
  requestPermissions as requestMicPermissions,
  checkPermissions as checkMicPermissions,
  openAppSettings
} from './index'

const invokeMock = vi.mocked(invoke)
const requestPermissionsMock = vi.mocked(requestPermissions)
const checkPermissionsMock = vi.mocked(checkPermissions)
const addPluginListenerMock = vi.mocked(addPluginListener)

function setUserAgent(ua: string): void {
  vi.stubGlobal('navigator', { userAgent: ua })
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('platform detection', () => {
  it('detects mobile user agents', () => {
    setUserAgent('Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36')
    expect(isMobile()).toBe(true)
    setUserAgent('Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)')
    expect(isMobile()).toBe(true)
    setUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')
    expect(isMobile()).toBe(false)
  })
})

describe('format mapping', () => {
  it('maps mime types to audio formats', () => {
    expect(formatFromMimeType('audio/mp4')).toBe('aac')
    expect(formatFromMimeType('audio/webm;codecs=opus')).toBe('opus')
    expect(formatFromMimeType('audio/wav')).toBe('wav')
    expect(formatFromMimeType('audio/flac')).toBe('flac')
    expect(formatFromMimeType(undefined)).toBe('unknown')
  })

  it('resolves preferred to effective format with wav fallback', () => {
    expect(resolveOutputFormat('m4a', ['wav', 'm4a'])).toBe('m4a')
    expect(resolveOutputFormat('flac', ['wav', 'm4a'])).toBe('wav')
    expect(resolveOutputFormat(undefined, ['wav'])).toBe('wav')
  })
})

describe('mobile dispatch', () => {
  beforeEach(() => {
    setUserAgent('Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36')
  })

  it('startRecording invokes the mobile command with options', async () => {
    invokeMock.mockResolvedValue(undefined)
    await startRecording({ format: 'wav', maxDuration: 5 })
    expect(invokeMock).toHaveBeenCalledWith('plugin:microphone-kessdev|start_recording', {
      options: { format: 'wav', maxDuration: 5 }
    })
  })

  it('stopRecording returns the mobile clip', async () => {
    invokeMock.mockResolvedValue({ path: '/tmp/a.wav', duration: 1.5, format: 'wav' })
    const clip = await stopRecording()
    expect(invokeMock).toHaveBeenCalledWith('plugin:microphone-kessdev|stop_recording')
    expect(clip.path).toBe('/tmp/a.wav')
    expect(clip.format).toBe('wav')
  })

  it('pause, resume and state invoke the mobile commands', async () => {
    invokeMock.mockResolvedValue(undefined)
    await pauseRecording()
    await resumeRecording()
    invokeMock.mockResolvedValue('paused')
    const state = await getRecordingState()
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'plugin:microphone-kessdev|pause_recording')
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'plugin:microphone-kessdev|resume_recording')
    expect(invokeMock).toHaveBeenNthCalledWith(3, 'plugin:microphone-kessdev|get_recording_state')
    expect(state).toBe('paused')
  })

  it('getCapabilities returns the mobile capabilities', async () => {
    invokeMock.mockResolvedValue({ formats: ['wav'], preferredFormat: 'wav' })
    const caps = await getCapabilities()
    expect(invokeMock).toHaveBeenCalledWith('plugin:microphone-kessdev|get_capabilities')
    expect(caps.preferredFormat).toBe('wav')
  })

  it('requestPermissions delegates to the core helper', async () => {
    requestPermissionsMock.mockResolvedValue({ microphone: 'granted' })
    const state = await requestMicPermissions()
    expect(requestPermissionsMock).toHaveBeenCalledWith('microphone-kessdev')
    expect(state).toBe('granted')
  })

  it('checkPermissions delegates to the core helper', async () => {
    checkPermissionsMock.mockResolvedValue({ microphone: 'denied' })
    const state = await checkMicPermissions()
    expect(checkPermissionsMock).toHaveBeenCalledWith('microphone-kessdev')
    expect(state).toBe('denied')
  })
})

describe('metering', () => {
  beforeEach(() => {
    setUserAgent('Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36')
  })

  it('subscribes, dispatches and cancels on mobile', async () => {
    const unregister = vi.fn()
    let captured: any = null
    addPluginListenerMock.mockImplementation(((_plugin: string, _event: string, cb: any) => {
      captured = cb
      return Promise.resolve({ unregister })
    }) as any)
    invokeMock.mockResolvedValue(undefined)

    const callback = vi.fn()
    const unlisten = onMeter(callback, { intervalMs: 50 })

    expect(invokeMock).toHaveBeenCalledWith('plugin:microphone-kessdev|start_meter', {
      options: { intervalMs: 50 }
    })
    expect(addPluginListenerMock).toHaveBeenCalledWith(
      'microphone-kessdev',
      'meter',
      callback
    )

    await Promise.resolve()
    captured({ level: 0.5, timestamp: 1 })
    expect(callback).toHaveBeenCalledWith({ level: 0.5, timestamp: 1 })

    unlisten()
    expect(unregister).toHaveBeenCalled()
    expect(invokeMock).toHaveBeenCalledWith('plugin:microphone-kessdev|stop_meter')
  })
})

describe('audio streaming', () => {
  it('normalizes raw desktop frames', async () => {
    setUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')
    invokeMock.mockResolvedValue({
      transport: 'raw',
      sampleRate: 48000,
      channels: 1,
      frameSamples: 1024
    })
    const chunks: any[] = []
    const stream = await startAudioStream(undefined, (chunk) => chunks.push(chunk))
    expect(stream.info.transport).toBe('raw')

    const channel = (globalThis as any).__lastChannel
    const buffer = new ArrayBuffer(20)
    const view = new DataView(buffer)
    view.setBigUint64(0, 5n, true)
    view.setFloat64(8, 1.5, true)
    new Uint8Array(buffer, 16).set([1, 2, 3, 4])
    channel.onmessage(buffer)

    expect(chunks[0].sequence).toBe(5)
    expect(chunks[0].timestamp).toBe(1.5)
    expect(chunks[0].data.byteLength).toBe(4)
  })

  it('normalizes base64 mobile frames and detects gaps', async () => {
    setUserAgent('Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36')
    invokeMock.mockResolvedValue({
      transport: 'base64',
      sampleRate: 48000,
      channels: 1,
      frameSamples: 1024
    })
    const chunks: any[] = []
    await startAudioStream({}, (chunk) => chunks.push(chunk))
    const channel = (globalThis as any).__lastChannel

    channel.onmessage({ sequence: 1, timestamp: 0.1, data: btoa('\x01\x02') })
    channel.onmessage({ sequence: 4, timestamp: 0.2, data: btoa('\x03') })

    expect(chunks[0].data.byteLength).toBe(2)
    expect(chunks[1].data.byteLength).toBe(1)
    expect(chunks[1].droppedBefore).toBe(2)
  })

  it('stop closes the stream', async () => {
    setUserAgent('Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36')
    invokeMock.mockResolvedValue({
      transport: 'base64',
      sampleRate: 48000,
      channels: 1,
      frameSamples: 1024
    })
    const stream = await startAudioStream({}, () => {})
    invokeMock.mockClear()
    await stream.stop()
    expect(invokeMock).toHaveBeenCalledWith('plugin:microphone-kessdev|stop_audio_stream')

    invokeMock.mockClear()
    await stopAudioStream()
    expect(invokeMock).toHaveBeenCalledWith('plugin:microphone-kessdev|stop_audio_stream')
  })
})

describe('desktop capabilities', () => {
  beforeEach(() => {
    setUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')
  })

  it('advertises flac and opus and resolves them when requested', async () => {
    const caps = await getCapabilities()
    expect(caps.formats).toContain('flac')
    expect(caps.formats).toContain('opus')
    expect(resolveOutputFormat('flac', caps.formats)).toBe('flac')
    expect(resolveOutputFormat('opus', caps.formats)).toBe('opus')
  })
})

describe('desktop degradation', () => {
  beforeEach(() => {
    setUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')
  })

  it('openAppSettings throws on desktop', async () => {
    await expect(openAppSettings()).rejects.toThrow(/not supported on desktop/)
  })

  it('rejects background recording on desktop', async () => {
    await expect(startRecording({ background: true })).rejects.toThrow(
      /Background recording is not supported on desktop/
    )
  })

  it('getRecordingState falls back to idle when the backend is unavailable', async () => {
    invokeMock.mockRejectedValue(new Error('unsupported'))
    const state = await getRecordingState()
    expect(state).toBe('idle')
  })
})

describe('desktop clip contract', () => {
  class FakeMediaRecorder {
    state = 'inactive'
    mimeType = 'audio/mp4'
    ondataavailable: ((event: { data: Blob }) => void) | null = null
    onstop: (() => void) | null = null
    onerror: (() => void) | null = null
    constructor(public stream: unknown) {}
    start(): void {
      this.state = 'recording'
    }
    stop(): void {
      this.state = 'inactive'
      this.ondataavailable?.({ data: new Blob(['pcm'], { type: this.mimeType }) })
      this.onstop?.()
    }
    pause(): void {
      this.state = 'paused'
    }
    resume(): void {
      this.state = 'recording'
    }
  }

  beforeEach(() => {
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
      mediaDevices: { getUserMedia: vi.fn().mockResolvedValue({ getTracks: () => [] }) }
    })
    vi.stubGlobal('MediaRecorder', FakeMediaRecorder)
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:test')
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
  })

  it('returns blob and url on desktop', async () => {
    await startRecording()
    const clip = await stopRecording()
    expect(clip.blob).toBeInstanceOf(Blob)
    expect(clip.url).toBe('blob:test')
    expect(clip.format).toBe('aac')
    expect(clip.duration).toBeGreaterThanOrEqual(0)
  })

  it('auto-stops at maxDuration and returns the finalized clip', async () => {
    vi.useFakeTimers()
    try {
      await startRecording({ maxDuration: 0.1 })
      await vi.advanceTimersByTimeAsync(200)
      const clip = await stopRecording()
      expect(clip.blob).toBeInstanceOf(Blob)
      expect(clip.url).toBe('blob:test')
    } finally {
      vi.useRealTimers()
    }
  })
})
