import { Show } from "solid-js";
import { View } from "@pocketjs/framework/components";
import {
  crosshairBars, crosshairColor, crosshairDot, crosshairDotSize, crosshairGap,
  crosshairLength, crosshairOutline, crosshairOutlineColor, crosshairOutlineSize,
  crosshairThickness, crosshairTilt,
} from "./player-settings.ts";

export default function PlayerCrosshair(props: { scale: number; box?: number }) {
  const box = () => props.box ?? 36;
  const c = () => box() * props.scale / 2;
  return <View style={{ width: box() * props.scale, height: box() * props.scale }}>
    <View class="absolute inset-0" style={{ rotate: crosshairTilt() }}>
      <Show when={crosshairBars()}>
        <Show when={crosshairOutline()}>
          <Bars center={c()} color={crosshairOutlineColor()} gap={(crosshairGap() - crosshairOutlineSize()) * props.scale} len={(crosshairLength() + crosshairOutlineSize() * 2) * props.scale} thick={(crosshairThickness() + crosshairOutlineSize() * 2) * props.scale} />
        </Show>
        <Bars center={c()} color={crosshairColor()} gap={crosshairGap() * props.scale} len={crosshairLength() * props.scale} thick={crosshairThickness() * props.scale} />
      </Show>
      <Show when={crosshairDot()}>
        <Show when={crosshairOutline()}>
          <Dot center={c()} size={(crosshairDotSize() + crosshairOutlineSize() * 2) * props.scale} color={crosshairOutlineColor()} />
        </Show>
        <Dot center={c()} size={crosshairDotSize() * props.scale} color={crosshairColor()} />
      </Show>
    </View>
  </View>;
}

function Dot(props: { center: number; size: number; color: string }) {
  return <View class="absolute" style={{ insetL: props.center - props.size / 2, insetT: props.center - props.size / 2, width: props.size, height: props.size, bgColor: props.color }} />;
}
function Bars(props: { center: number; color: string; gap: number; len: number; thick: number }) {
  const bar = (x: number, y: number, w: number, h: number) => <View class="absolute" style={{ insetL: x, insetT: y, width: w, height: h, bgColor: props.color }} />;
  return <View class="absolute inset-0">
    {bar(props.center - props.thick / 2, props.center - props.gap / 2 - props.len, props.thick, props.len)}
    {bar(props.center - props.thick / 2, props.center + props.gap / 2, props.thick, props.len)}
    {bar(props.center - props.gap / 2 - props.len, props.center - props.thick / 2, props.len, props.thick)}
    {bar(props.center + props.gap / 2, props.center - props.thick / 2, props.len, props.thick)}
  </View>;
}
