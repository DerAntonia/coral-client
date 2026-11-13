# Coral Client vs. Horizon, 2026-11-13

![Test world](mangohud-2026-11-13/perfwelt.png)

Same test world, same spot, same settings, uncapped, measured 30 s with MangoHud.
Ryzen 7 9700X, RX 9060 XT, Linux.

| Client | Avg FPS | 1 % low | 0.1 % low |
|---|---|---|---|
| Horizon | 799 | 414 | 272 |
| **Coral** | **969** | **577** | **384** |
| Fabulously Optimized (Xmx4096) | 571 | 214 | 155 |

| Client | RAM | VRAM |
|---|---|---|
| Horizon | 832 MiB | 396 MiB |
| Coral | 957 MiB | 658 MiB |

| Client | GPU load | CPU load (total) |
|---|---|---|
| Horizon | 55 % | 13 % |
| Coral | 69 % | 11 % |
| Fabulously Optimized | 50 % | 12 % |

Coral with all motion blur on (hand, entities, camera): 505 FPS avg, 271 FPS 1 % low.

A YouTube video was playing in the background during the measurements.

Logs: `mangohud-2026-11-13/`
