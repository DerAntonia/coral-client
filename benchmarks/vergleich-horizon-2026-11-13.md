# Coral Client vs. Horizon, 2026-11-13

![Testwelt](mangohud-2026-11-13/perfwelt.png)

Gleiche Testwelt, gleicher Standort, gleiche Einstellungen, ohne FPS-Begrenzung, 30 s gemessen
mit MangoHud. Ryzen 7 9700X, RX 9060 XT, Linux.

| Client | Ø FPS | 1-%-Low | 0,1-%-Low |
|---|---|---|---|
| Horizon | 799 | 414 | 272 |
| **Coral** | **969** | **577** | **384** |

| Client | RAM | VRAM |
|---|---|---|
| Horizon | 832 MiB | 396 MiB |
| Coral | 957 MiB | 658 MiB |

Coral mit allem Motion Blur an (Hand, Entities, Kamera): 505 FPS im Schnitt, 271 FPS 1-%-Low.

Logs: `mangohud-2026-11-13/`
