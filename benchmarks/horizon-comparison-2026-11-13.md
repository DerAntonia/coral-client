# Coral Client vs. Horizon, 2026-11-13

![Test world](mangohud-2026-11-13/perfwelt.png)

Same test world, same spot, same settings, uncapped, measured 30 s with MangoHud.
Ryzen 7 9700X, RX 9060 XT, Linux.

| Client | Avg FPS | 1 % low | 0.1 % low |
|---|---|---|---|
| Horizon | 799 | 414 | 272 |
| **Coral** | **969** | **577** | **384** |

| Client | RAM | VRAM |
|---|---|---|
| Horizon | 832 MiB | 396 MiB |
| Coral | 957 MiB | 658 MiB |

Coral with all motion blur on (hand, entities, camera): 505 FPS avg, 271 FPS 1 % low.

Logs: `mangohud-2026-11-13/`
