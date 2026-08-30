import { useState } from 'react'

import { useT } from '../i18n'
import { Button, Icon } from '../ui'
import { copyText } from '../lib/clipboard'

/**
 * Installing the Word add-in.
 *
 * Office sideloading is a file-in-a-folder ritual, and the folder differs per
 * platform. Rather than write a paragraph of instructions the reader has to
 * translate into their own case, this hands over the two things that are
 * actually needed — the manifest, and the one path it goes in — and picks the
 * path from the browser's own platform.
 *
 * The manifest is downloaded rather than linked because Office wants a file on
 * disk; and it is fetched from this origin so the URLs inside it name the host
 * the author is already talking to. See `addin/mod.rs` for why it is generated
 * per request.
 */
export function WordAddin() {
  const t = useT()
  const [copied, setCopied] = useState(false)
  const platform = detectPlatform()

  const copy = async () => {
    try {
      await copyText(SIDELOAD[platform].path)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      setCopied(false) // A denied clipboard is not worth an error; the path is on screen.
    }
  }

  return (
    <div className="addin-install">
      <div className="button-row">
        <Button onClick={() => window.open('/addin/manifest.xml', '_blank')}>
          <Icon.Download size={12} />
          {t('addin.download')}
        </Button>
        <Button onClick={copy} title={SIDELOAD[platform].path}>
          <Icon.Copy size={12} />
          {copied ? t('addin.copied') : t('addin.copyPath')}
        </Button>
      </div>
      <ol className="addin-steps">
        <li>{t('addin.step1')}</li>
        <li>
          {t(`addin.step2.${platform}`)} <code>{SIDELOAD[platform].path}</code>
        </li>
        <li>{t('addin.step3')}</li>
      </ol>
    </div>
  )
}

type Platform = 'windows' | 'mac' | 'other'

/**
 * Which sideload folder to show.
 *
 * The browser cannot know where Word is installed, so this is a guess from the
 * user agent — but showing the likely path is far more use than showing all
 * three and asking the reader to work out which one they are.
 */
export function detectPlatform(agent = navigator.userAgent): Platform {
  if (/Win/i.test(agent)) return 'windows'
  if (/Mac|iPhone|iPad/i.test(agent)) return 'mac'
  return 'other'
}

export const SIDELOAD: Record<Platform, { path: string }> = {
  // A network share, because that is what Word's trusted-catalogue setting
  // takes on Windows; a plain folder will not appear in the Ribbon.
  windows: { path: '\\\\localhost\\c$\\yinkote-addin' },
  mac: { path: '~/Library/Containers/com.microsoft.Word/Data/Documents/wef' },
  other: { path: '~/wef' },
}
