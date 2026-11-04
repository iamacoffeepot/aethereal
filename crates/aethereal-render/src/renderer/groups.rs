//! The bind groups a placement, a dispatch, or an effect binds, with what each holds, so a texture made again at
//! another size is bound again where it was bound (ADR-0049).

/// Bind groups made through `layouts`, the first of the group `first`, each binding what `entries` holds in it.
pub(super) struct Groups {
    layouts: Vec<wgpu::BindGroupLayout>,
    first: u32,
    pub entries: Vec<Entry>,
    made: Vec<wgpu::BindGroup>,
}

/// What one binding of a group holds.
pub(super) struct Entry {
    pub group: u32,
    pub binding: u32,
    pub holds: Binds,
}

/// A resource a binding holds: a buffer of uniforms or storage, a texture, by its id, through its view, or a sampler.
pub(super) enum Binds {
    Buffer(wgpu::Buffer),
    Texture { texture: u64, view: wgpu::TextureView },
    Sampler(wgpu::Sampler),
}

impl Groups {
    /// A group for each of `layouts`, the first of the group `first`, binding what `entries` holds in it.
    pub fn new(device: &wgpu::Device, layouts: Vec<wgpu::BindGroupLayout>, first: u32, entries: Vec<Entry>) -> Self {
        let made = made(device, &layouts, first, &entries);

        Self { layouts, first, entries, made }
    }

    /// Each group, beside the number it is bound at.
    pub fn each(&self) -> impl Iterator<Item = (u32, &wgpu::BindGroup)> {
        (self.first..).zip(&self.made)
    }

    /// Binds `view` where the texture `texture` was bound, and makes the groups again, when any binds it.
    pub fn rebind(&mut self, device: &wgpu::Device, texture: u64, view: &wgpu::TextureView) {
        let mut binds = false;
        for entry in &mut self.entries {
            if let Binds::Texture { texture: bound, view: held } = &mut entry.holds
                && *bound == texture
            {
                *held = view.clone();
                binds = true;
            }
        }
        if binds {
            self.made = made(device, &self.layouts, self.first, &self.entries);
        }
    }
}

/// A group for each of `layouts`, the first of the group `first`, binding what `entries` holds in it.
fn made(
    device: &wgpu::Device,
    layouts: &[wgpu::BindGroupLayout],
    first: u32,
    entries: &[Entry],
) -> Vec<wgpu::BindGroup> {
    (first..)
        .zip(layouts)
        .map(|(group, layout)| {
            let bound: Vec<wgpu::BindGroupEntry<'_>> = entries
                .iter()
                .filter(|entry| entry.group == group)
                .map(|entry| {
                    let resource = match &entry.holds {
                        Binds::Buffer(buffer) => buffer.as_entire_binding(),
                        Binds::Texture { view, .. } => wgpu::BindingResource::TextureView(view),
                        Binds::Sampler(sampler) => wgpu::BindingResource::Sampler(sampler),
                    };
                    wgpu::BindGroupEntry { binding: entry.binding, resource }
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &bound })
        })
        .collect()
}
