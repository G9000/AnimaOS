import discord from '../assets/brands/discord.svg';
import gmail from '../assets/brands/gmail.svg';
import googleCalendar from '../assets/brands/google-calendar.svg';
import googleDrive from '../assets/brands/google-drive.svg';
import onedrive from '../assets/brands/onedrive.svg';
import outlook from '../assets/brands/outlook.svg';
import slack from '../assets/brands/slack.svg';
import telegram from '../assets/brands/telegram.svg';
import whatsapp from '../assets/brands/whatsapp.svg';

const BRANDS = {
  discord,
  gmail,
  'google-calendar': googleCalendar,
  'google-drive': googleDrive,
  onedrive,
  outlook,
  slack,
  telegram,
  whatsapp,
};

export type Brand = keyof typeof BRANDS;

/** A service's own mark, from `assets/brands`. Decorative: the name beside
 *  it is what a screen reader hears. */
export function BrandIcon({
  brand,
  size = 22,
}: {
  brand: Brand;
  size?: number;
}) {
  return (
    <img
      src={BRANDS[brand]}
      width={size}
      height={size}
      alt=""
      aria-hidden
      draggable={false}
    />
  );
}
