import {
  invoke,
  convertFileSrc,
  requestPermissions as requestPermissions_,
  checkPermissions as checkPermissions_,
  addPluginListener,
  Channel,
  type PermissionState as CorePermissionState
} from '@tauri-apps/api/core'

export { convertFileSrc }

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export type AudioFormat = 'wav' | 'm4a' | 'aac' | 'flac' | 'opus'
export type RecordingState = 'idle' | 'recording' | 'paused'
export type PermissionState = CorePermissionState
export type MeterScale = 'linear' | 'dbfs'
export type StreamTransport = 'raw' | 'base64'

export interface RecordingOptions {
  /** Preferred output format. The effective format is reported in `AudioClip`. */
  format?: AudioFormat
  sampleRate?: number
  channels?: number
  bitRate?: number
  /** Automatic stop after this many seconds. */
  maxDuration?: number
  /** Desktop input device identifier. */
  deviceId?: string
  /** Request continued recording in the background (mobile only). */
  background?: boolean
}

export interface AudioClip {
  /** Duration in seconds. */
  duration: number
  /** Format actually produced (may differ from the requested one). */
  format: AudioFormat | string
  /** Absolute path to a temporary file (mobile, and desktop native path). */
  path?: string
  /** Recorded audio blob (desktop). */
  blob?: Blob
  /** Object URL for `blob` (desktop). Remember to revoke it. */
  url?: string
}

export interface MeterOptions {
  intervalMs?: number
  scale?: MeterScale
  floorDb?: number
  peakHold?: boolean
  spectrum?: boolean
}

export interface MeterData {
  /** Normalized level. */
  level: number
  peak?: number
  rms?: number
  db?: number
  clipping?: boolean
  spectrum?: number[]
  timestamp: number
}

export interface Capabilities {
  formats: AudioFormat[]
  preferredFormat: AudioFormat
  sampleRates: number[]
  channels: number[]
  supportsPause: boolean
  supportsBackground: boolean
  supportsSpectrum: boolean
  supportsAudioStream: boolean
  streamTransport: StreamTransport
}

export interface StreamOptions {
  sampleRate?: number
  channels?: number
  frameSamples?: number
  maxSeconds?: number
}

export interface StreamInfo {
  transport: StreamTransport
  sampleRate: number
  channels: number
  frameSamples: number
}

export interface AudioChunk {
  /** PCM payload. */
  data: ArrayBuffer
  sequence: number
  timestamp: number
  droppedBefore?: number
}

export type UnlistenFn = () => void

export interface AudioStream {
  info: StreamInfo
  stop: () => Promise<void>
}

const STREAM_HEADER_BYTES = 16 // [sequence: u64][timestamp: f64]

const PLUGIN = 'microphone-kessdev'

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

/**
 * Detects whether the current runtime is a Tauri mobile webview (Android/iOS).
 * Desktop runtimes fall through to the webview backend.
 */
export function isMobile(): boolean {
  if (typeof navigator === 'undefined') return false
  const ua = navigator.userAgent
  return /android/i.test(ua) || /iphone|ipad|ipod/i.test(ua)
}

/** Maps a MediaRecorder MIME type to the closest `AudioFormat`. */
export function formatFromMimeType(mimeType: string | undefined): AudioFormat | string {
  const type = (mimeType ?? '').toLowerCase()
  if (type.includes('wav')) return 'wav'
  if (type.includes('flac')) return 'flac'
  if (type.includes('aac') || type.includes('m4a') || type.includes('mp4')) return 'aac'
  if (type.includes('opus') || type.includes('ogg') || type.includes('webm')) return 'opus'
  return 'unknown'
}

/**
 * Resolves the format to request when the caller expresses a preference: keeps
 * the requested format when the platform supports it, otherwise falls back to
 * the always-available `wav`.
 */
export function resolveOutputFormat(
  requested: AudioFormat | undefined,
  supported: AudioFormat[]
): AudioFormat {
  if (requested && supported.includes(requested)) return requested
  return 'wav'
}

const NATIVE_DESKTOP_FORMATS: AudioFormat[] = ['wav', 'flac', 'opus']

function usesNativeDesktopPath(options?: RecordingOptions): boolean {
  return options?.format !== undefined && NATIVE_DESKTOP_FORMATS.includes(options.format)
}

// ---------------------------------------------------------------------------
// Desktop backend state (getUserMedia + MediaRecorder + AnalyserNode)
// ---------------------------------------------------------------------------

let activeStream: MediaStream | null = null
let mediaRecorder: MediaRecorder | null = null
let recordedChunks: Blob[] = []
let recordingStartedAt = 0
let desktopState: RecordingState = 'idle'
let pendingDesktopClip: AudioClip | null = null
let desktopAutoStopTimer: ReturnType<typeof setTimeout> | null = null
let audioContext: AudioContext | null = null
let analyser: AnalyserNode | null = null
let meterTimer: ReturnType<typeof setInterval> | null = null

async function ensureDesktopStream(options?: RecordingOptions): Promise<MediaStream> {
  if (activeStream) return activeStream

  const audioConstraints: MediaTrackConstraints = {}
  if (options?.deviceId) audioConstraints.deviceId = { exact: options.deviceId }
  if (options?.sampleRate) audioConstraints.sampleRate = options.sampleRate
  if (options?.channels) audioConstraints.channelCount = options.channels

  const stream = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints })
  activeStream = stream

  const AudioCtor = (globalThis as any).AudioContext || (globalThis as any).webkitAudioContext
  if (typeof AudioCtor === 'function') {
    const ctx: AudioContext = new AudioCtor()
    const source = ctx.createMediaStreamSource(stream)
    const node = ctx.createAnalyser()
    node.fftSize = 2048
    source.connect(node)
    audioContext = ctx
    analyser = node
  }

  return stream
}

async function materializeNativeClip(clip: AudioClip): Promise<AudioClip> {
  if (!clip.path) return clip
  try {
    const response = await fetch(convertFileSrc(clip.path))
    const blob = await response.blob()
    return { ...clip, blob, url: URL.createObjectURL(blob) }
  } catch {
    return clip
  }
}

// ---------------------------------------------------------------------------
// Recording lifecycle
// ---------------------------------------------------------------------------

/** Starts recording audio from the microphone. */
export async function startRecording(options?: RecordingOptions): Promise<void> {
  if (isMobile()) {
    await invoke(`plugin:${PLUGIN}|start_recording`, { options: options ?? {} })
    return
  }

  if (options?.background) {
    throw new Error('Background recording is not supported on desktop.')
  }

  if (usesNativeDesktopPath(options)) {
    await invoke(`plugin:${PLUGIN}|start_recording`, { options: options ?? {} })
    desktopState = 'recording'
    return
  }

  const stream = await ensureDesktopStream(options)
  recordedChunks = []
  const recorder = new MediaRecorder(stream)
  recorder.ondataavailable = (event) => {
    if (event.data.size > 0) recordedChunks.push(event.data)
  }
  mediaRecorder = recorder
  recordingStartedAt = performance.now()
  recorder.start()
  desktopState = 'recording'

  if (options?.maxDuration && options.maxDuration > 0) {
    desktopAutoStopTimer = setTimeout(() => {
      void stopRecording().then((clip) => {
        pendingDesktopClip = clip
      })
    }, options.maxDuration * 1000)
  }
}

/** Stops recording and returns the resulting clip. */
export async function stopRecording(): Promise<AudioClip> {
  if (isMobile()) {
    return await invoke<AudioClip>(`plugin:${PLUGIN}|stop_recording`)
  }

  if (pendingDesktopClip) {
    const clip = pendingDesktopClip
    pendingDesktopClip = null
    return clip
  }

  if (desktopAutoStopTimer) {
    clearTimeout(desktopAutoStopTimer)
    desktopAutoStopTimer = null
  }

  if (desktopState === 'recording' && mediaRecorder) {
    const duration = (performance.now() - recordingStartedAt) / 1000
    const recorder = mediaRecorder
    return await new Promise<AudioClip>((resolve, reject) => {
      recorder.onstop = () => {
        const type = recorder.mimeType || 'audio/webm'
        const blob = new Blob(recordedChunks, { type })
        const url = URL.createObjectURL(blob)
        recordedChunks = []
        mediaRecorder = null
        desktopState = 'idle'
        if (activeStream) activeStream.getTracks().forEach((t) => t.stop())
        activeStream = null
        if (audioContext) {
          void audioContext.close()
          audioContext = null
        }
        analyser = null
        resolve({ duration, format: formatFromMimeType(type), blob, url })
      }
      recorder.onerror = () => reject(new Error('Audio recording failed.'))
      recorder.stop()
    })
  }

  const clip = await invoke<AudioClip>(`plugin:${PLUGIN}|stop_recording`)
  desktopState = 'idle'
  return materializeNativeClip(clip)
}

/** Pauses an ongoing recording. */
export async function pauseRecording(): Promise<void> {
  if (isMobile()) {
    await invoke(`plugin:${PLUGIN}|pause_recording`)
    return
  }
  if (mediaRecorder && mediaRecorder.state === 'recording') {
    mediaRecorder.pause()
  } else {
    await invoke(`plugin:${PLUGIN}|pause_recording`)
  }
  desktopState = 'paused'
}

/** Resumes a paused recording. */
export async function resumeRecording(): Promise<void> {
  if (isMobile()) {
    await invoke(`plugin:${PLUGIN}|resume_recording`)
    return
  }
  if (mediaRecorder && mediaRecorder.state === 'paused') {
    mediaRecorder.resume()
  } else {
    await invoke(`plugin:${PLUGIN}|resume_recording`)
  }
  desktopState = 'recording'
}

/** Returns the current recording state. */
export async function getRecordingState(): Promise<RecordingState> {
  if (isMobile()) {
    return await invoke<RecordingState>(`plugin:${PLUGIN}|get_recording_state`)
  }
  if (desktopState !== 'idle') return desktopState
  try {
    return await invoke<RecordingState>(`plugin:${PLUGIN}|get_recording_state`)
  } catch {
    return 'idle'
  }
}

// ---------------------------------------------------------------------------
// Raw audio streaming
// ---------------------------------------------------------------------------

function base64ToArrayBuffer(base64: string): ArrayBuffer {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i)
  }
  return bytes.buffer
}

interface RawFrame {
  data?: string
  sequence?: number
  timestamp?: number
  droppedBefore?: number
}

/**
 * Starts a raw PCM stream. Frames are delivered to `onChunk` in order, each with
 * a sequence number and timestamp, until the stream is stopped (or `maxSeconds`
 * is reached). Desktop delivers raw `ArrayBuffer` frames; mobile delivers base64
 * payloads, both normalized to `AudioChunk`.
 */
export async function startAudioStream(
  options: StreamOptions | undefined,
  onChunk: (chunk: AudioChunk) => void
): Promise<AudioStream> {
  const channel = new Channel<ArrayBuffer | RawFrame>()
  let lastSequence = -1

  channel.onmessage = (message: ArrayBuffer | RawFrame) => {
    if (message instanceof ArrayBuffer) {
      const view = new DataView(message)
      const sequence = Number(view.getBigUint64(0, true))
      const timestamp = view.getFloat64(8, true)
      const data = message.slice(STREAM_HEADER_BYTES)
      const droppedBefore =
        lastSequence >= 0 && sequence > lastSequence + 1
          ? sequence - lastSequence - 1
          : undefined
      lastSequence = sequence
      onChunk({ data, sequence, timestamp, droppedBefore })
      return
    }
    const sequence = message.sequence ?? 0
    const data = message.data ? base64ToArrayBuffer(message.data) : new ArrayBuffer(0)
    const droppedBefore =
      message.droppedBefore ??
      (lastSequence >= 0 && sequence > lastSequence + 1
        ? sequence - lastSequence - 1
        : undefined)
    lastSequence = sequence
    onChunk({ data, sequence, timestamp: message.timestamp ?? 0, droppedBefore })
  }

  const info = await invoke<StreamInfo>(`plugin:${PLUGIN}|start_audio_stream`, {
    options: options ?? {},
    channel
  })
  return { info, stop: () => stopAudioStream() }
}

/** Stops the active raw audio stream and closes its `Channel`. */
export async function stopAudioStream(): Promise<void> {
  await invoke(`plugin:${PLUGIN}|stop_audio_stream`)
}

// ---------------------------------------------------------------------------
// Metering
// ---------------------------------------------------------------------------

/**
 * Subscribes to live audio level updates. Returns a function that cancels the
 * subscription.
 */
export function onMeter(callback: (data: MeterData) => void, options?: MeterOptions): UnlistenFn {
  if (isMobile()) {
    let cancelled = false
    let unregister: (() => void) | null = null
    void invoke(`plugin:${PLUGIN}|start_meter`, { options: options ?? {} }).catch(() => {})
    void addPluginListener<MeterData>(PLUGIN, 'meter', callback).then((listener) => {
      if (cancelled) {
        void listener.unregister()
      } else {
        unregister = () => void listener.unregister()
      }
    })
    return () => {
      cancelled = true
      unregister?.()
      void invoke(`plugin:${PLUGIN}|stop_meter`).catch(() => {})
    }
  }

  const interval = options?.intervalMs ?? 100
  const floorDb = options?.floorDb ?? -60
  const scale = options?.scale ?? 'linear'
  const buffer = analyser ? new Float32Array(analyser.fftSize) : null

  if (meterTimer) clearInterval(meterTimer)
  meterTimer = setInterval(() => {
    if (!analyser || !buffer) return
    analyser.getFloatTimeDomainData(buffer)
    let peak = 0
    let sumSquares = 0
    for (let i = 0; i < buffer.length; i++) {
      const sample = Math.abs(buffer[i])
      if (sample > peak) peak = sample
      sumSquares += buffer[i] * buffer[i]
    }
    const rms = Math.sqrt(sumSquares / buffer.length)
    const toDb = (value: number) => (value <= 0 ? floorDb : Math.max(20 * Math.log10(value), floorDb))
    const level = scale === 'dbfs' ? toDb(peak) : Math.min(peak, 1)
    callback({
      level,
      peak: Math.min(peak, 1),
      rms: Math.min(rms, 1),
      db: toDb(peak),
      clipping: peak >= 1,
      timestamp: Date.now() / 1000
    })
  }, interval)

  return () => {
    if (meterTimer) {
      clearInterval(meterTimer)
      meterTimer = null
    }
  }
}

/**
 * Desktop only: returns the `AnalyserNode` attached to the active recording
 * stream so advanced visualizers can read raw frequency data. Returns `null` on
 * mobile and when no desktop stream is active.
 */
export function getAnalyserNode(): AnalyserNode | null {
  if (isMobile()) return null
  return analyser
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/** Queries the formats and features supported by the current platform. */
export async function getCapabilities(): Promise<Capabilities> {
  if (isMobile()) {
    return await invoke<Capabilities>(`plugin:${PLUGIN}|get_capabilities`)
  }
  const formats: AudioFormat[] = ['wav', 'flac', 'opus']
  if (typeof MediaRecorder !== 'undefined') {
    if (MediaRecorder.isTypeSupported('audio/mp4')) formats.push('m4a', 'aac')
    if (MediaRecorder.isTypeSupported('audio/webm;codecs=opus')) formats.push('opus')
  }
  return {
    formats,
    preferredFormat: 'wav',
    sampleRates: [44100, 48000],
    channels: [1, 2],
    supportsPause: true,
    supportsBackground: false,
    supportsSpectrum: true,
    supportsAudioStream: true,
    streamTransport: 'raw'
  }
}

/** Returns the list of output formats supported on the current platform. */
export async function getSupportedFormats(): Promise<AudioFormat[]> {
  if (isMobile()) {
    return await invoke<AudioFormat[]>(`plugin:${PLUGIN}|get_supported_formats`)
  }
  return (await getCapabilities()).formats
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

/** Gets the current microphone permission state. */
export async function checkPermissions(): Promise<PermissionState> {
  if (isMobile()) {
    return await checkPermissions_<{ microphone: PermissionState }>(PLUGIN).then((r) => r.microphone)
  }
  try {
    const status = await navigator.permissions.query({ name: 'microphone' as PermissionName })
    return status.state as PermissionState
  } catch {
    return 'prompt'
  }
}

/** Requests microphone permission. */
export async function requestPermissions(): Promise<PermissionState> {
  if (isMobile()) {
    return await requestPermissions_<{ microphone: PermissionState }>(PLUGIN).then((r) => r.microphone)
  }
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
    stream.getTracks().forEach((t) => t.stop())
    return 'granted'
  } catch {
    return 'denied'
  }
}

/** Opens the application settings screen (mobile only). */
export async function openAppSettings(): Promise<void> {
  if (isMobile()) {
    await invoke(`plugin:${PLUGIN}|open_app_settings`)
    return
  }
  throw new Error('openAppSettings is not supported on desktop.')
}
