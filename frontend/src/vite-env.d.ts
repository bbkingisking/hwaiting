/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_BUILD_HASH: string
}

declare module '*.lottie' {
  const content: any
  export default content
}
