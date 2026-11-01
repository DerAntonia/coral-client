// Oberflaeche, Frame-Latenz und Groesse.

use super::*;

impl GpuState {
    // surface.configure mit Rueckfall. wgpu 22 macht aus jedem Fehler dabei einen Panic; unter
    // Windows kann DXGI das Anlegen oder Anpassen der Swapchain aber ablehnen, etwa wenn ein
    // Aufnahme- oder Overlay-Programm (Medal, Discord-Stream) noch Bilder der alten haelt. Dann
    // eine frische Surface mit neuer Swapchain; klappt auch das nicht, bleibt das Bild stehen
    // und acquire_frame versucht es spaeter erneut, statt den Client zu beenden. Den Grund
    // schreibt wgpu ueber `log` ins client.log (s. WgpuLog).
    pub(crate) fn configure_surface(&mut self) -> bool {
        let versuch = |surface: &wgpu::Surface, device: &wgpu::Device, config: &wgpu::SurfaceConfiguration| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| surface.configure(device, config))).is_ok()
        };
        self.surface_ok = versuch(&self.surface, &self.device, &self.config);
        if !self.surface_ok {
            println!("[gpu] Swapchain liess sich nicht einrichten, lege die Surface neu an");
            match self.instance.create_surface(self.window.clone()) {
                // Erst zuweisen, dann einrichten: die alte Swapchain muss weg sein, ein Fenster
                // traegt nur eine.
                Ok(neu) => {
                    self.surface = neu;
                    self.surface_ok = versuch(&self.surface, &self.device, &self.config);
                }
                Err(e) => println!("[gpu] Surface neu anlegen fehlgeschlagen: {e}"),
            }
        }
        // Exklusives Vollbild stellt den Bildschirmmodus um; zusammen mit Aufnahme-Hooks lehnt
        // DXGI die Swapchain dann teils ab (DXGI_ERROR_INVALID_CALL). Randlos geht dort. Die
        // neue Groesse kommt per Resized (resize -> configure_surface), sonst greift der
        // naechste Versuch von acquire_frame.
        if !self.surface_ok && matches!(self.window.fullscreen(), Some(Fullscreen::Exclusive(_))) {
            println!("[gpu] Rueckfall auf randloses Vollbild");
            self.window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        if !self.surface_ok {
            println!("[gpu] Swapchain weiter nicht einrichtbar, neuer Versuch in 2 s");
            self.surface_retry_at = Instant::now() + Duration::from_secs(2);
        }
        self.surface_ok
    }

    pub(crate) fn gpu_label(&self) -> String {
        let vendor = match self.adapter_info.vendor {
            PCI_VENDOR_AMD => "AMD",
            PCI_VENDOR_NVIDIA => "Nvidia",
            PCI_VENDOR_INTEL => "Intel",
            _ => "?",
        };
        format!("{vendor}: {}", self.adapter_info.name)
    }

    // Taktet der Present-Modus schon selbst? Bei Fifo (und FifoRelaxed) blockiert
    // get_current_texture(), bis ein Swapchain-Bild frei wird -- der VBlank gibt den Takt
    // vor. Ein eigener Timer daneben ist dann nicht nur ueberfluessig, sondern schaedlich:
    // zwei unabhaengige Uhren schweben gegeneinander, wir verpassen mal einen VBlank und
    // draengeln mal zwei Frames. Immediate und Mailbox blockieren dagegen nicht und
    // brauchen unseren Deckel, sonst rendern sie ungebremst.
    pub(crate) const fn present_mode_self_paces(&self) -> bool {
        matches!(self.config.present_mode, wgpu::PresentMode::Fifo | wgpu::PresentMode::FifoRelaxed | wgpu::PresentMode::AutoVsync)
    }

    // CPU-Render-Ahead-Limit umschalten -- ein sofortiges configure(),
    // aber OHNE Treiber-Rueckfrage: wgpu akzeptiert jeden Wert >=1 auf jedem Backend, es gibt
    // also keine Liste "unterstuetzter" Werte wie bei present_modes.
    // value=0 ist der Sodium-Style-Zero-Modus (echte Fence-Wait vor jedem Frame statt eines
    // Swapchain-Parameters, s. fence_wait_zero-Feldkommentar) -- die Swapchain selbst bekommt
    // dabei trotzdem einen gueltigen Wert (1), wgpu wuerde eine 0 ohnehin nur darauf clampen.
    pub(crate) fn set_frame_latency(&mut self, value: u32) {
        let wants_zero_mode = value == 0;
        let swapchain_value = value.max(1);
        if self.config.desired_maximum_frame_latency != swapchain_value {
            self.config.desired_maximum_frame_latency = swapchain_value;
            self.configure_surface();
        }
        self.fence_wait_zero = wants_zero_mode;
        println!("[gpu] Render-Ahead-Limit -> {value} (Swapchain-Wert {swapchain_value}, Zero-Modus {})", self.fence_wait_zero);
    }

    // Am Anfang jedes Frames aufgerufen (vor get_current_texture()): im Zero-Modus wird hart
    // auf das GPU-Ende des VORHERIGEN Frames gewartet, bevor dieser Frame ueberhaupt anfaengt
    // -- damit steht nie mehr als das gerade angezeigte Frame in der Pipeline (Sodiums "CPU
    // Render-Ahead Limit: 0"). Ausserhalb des Zero-Modus ein No-Op (die Swapchain uebernimmt
    // die Drosselung selbst, s. desired_maximum_frame_latency).
    pub(crate) fn wait_for_previous_frame_if_zero_mode(&self) {
        if self.fence_wait_zero {
            if let Some(index) = &self.last_submission {
                self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(index.clone()));
            }
        }
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.configure_surface();
        self.block_renderer.resize(&self.device, width, height);
        if let Some(lod) = self.lod.as_mut() {
            lod.resize(&self.device, width, height);
        }
        let (offscreen_texture, offscreen_view) = create_offscreen_texture(&self.device, self.config.format, width, height);
        self.offscreen_texture = offscreen_texture;
        self.offscreen_view = offscreen_view;
        self.world_backdrop = None;
        self.offscreen_world_only = false;
        // Der Crosshair-Bind-Group haengt an der alten offscreen_view-Identitaet -- neu
        // erzeugen, sonst zeigt er nach einem Resize auf eine bereits zerstoerte Textur.
        self.crosshair_bind_group = create_crosshair_bind_group(
            &self.device,
            &self.crosshair_bind_group_layout,
            &self.offscreen_view,
            &self.crosshair_sampler,
            &self.crosshair_mode_buffer,
        );
        self.color_bind_group =
            create_color_bind_group(&self.device, &self.color_bind_group_layout, &self.offscreen_view, &self.color_settings_buffer);
        // CMAA2-Ausgabetextur haengt an der Bildschirmgroesse (s. create_cmaa2_output_texture)
        // und beide Bind-Groups an den konkreten View-Identitaeten -- alle drei bei jedem
        // resize() neu erzeugen, aus demselben Grund wie beim Crosshair oben.
        let (cmaa2_output_texture, cmaa2_output_view) = create_cmaa2_output_texture(&self.device, width, height);
        self.cmaa2_output_texture = cmaa2_output_texture;
        self.cmaa2_output_view = cmaa2_output_view;
        self.cmaa2_compute_bind_group = create_cmaa2_compute_bind_group(
            &self.device,
            &self.cmaa2_compute_bind_group_layout,
            &self.offscreen_view,
            &self.cmaa2_output_view,
        );
        self.cmaa2_blit_bind_group = create_cmaa2_blit_bind_group(
            &self.device,
            &self.cmaa2_blit_bind_group_layout,
            &self.cmaa2_output_view,
            &self.cmaa2_blit_sampler,
        );
    }

    pub(crate) const fn size(&self) -> (f32, f32) {
        (self.config.width as f32, self.config.height as f32)
    }
}
