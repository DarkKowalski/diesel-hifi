<script lang="ts">
  export type IconName = 'engine' | 'drive' | 'sound' | 'details' | 'power' | 'reset' | 'arrow' | 'globe' | 'mute';
  let { name, size = 20 }: { name: IconName; size?: number } = $props();
  const paths: Record<IconName, string> = {
    engine: 'M3 10h3l3-4h8v3h4v10H6v-3H3z M10 3h5 M12 3v3 M1 10v6 M10 10h5v5h-5z',
    drive: 'M4 18a9 9 0 1 1 16 0 M7 18h10 M12 14l5-7 M5 10l2 1 M12 3v3 M19 10l-2 1',
    sound: 'M4 9h4l5-4v14l-5-4H4z M17 8a6 6 0 0 1 0 8 M20 5a10 10 0 0 1 0 14',
    details: 'M4 6h16 M4 12h16 M4 18h16 M8 3v6 M16 9v6 M10 15v6',
    power: 'M12 3v9 M6.4 5.8a8 8 0 1 0 11.2 0',
    reset: 'M4 10a8 8 0 1 1 1 7 M4 4v6h6',
    arrow: 'M5 12h14 M14 7l5 5-5 5',
    globe: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0 M3 12h18 M12 3c5 5 5 13 0 18-5-5-5-13 0-18',
    mute: 'M4 9h4l5-4v14l-5-4H4z M17 9l5 6 M22 9l-5 6',
  };
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>
