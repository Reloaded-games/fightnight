use super::Renderer;
use fn_core::mesh::Vertex;

impl Renderer {
    /// Update just the playable skin's existing vertex range, retaining all other mesh buffers.
    pub fn update_skinned_mesh(&self,index:usize,vertices:&[Vertex]) {
        let Some(range)=self.mesh_table.get(index) else{return;};
        if range.vertex_count as usize!=vertices.len(){return;}
        if let Some(buffer)=&self.mesh_vb {
            self.gpu.queue.write_buffer(buffer,range.base_vertex as u64*std::mem::size_of::<Vertex>() as u64,bytemuck::cast_slice(vertices));
        }
    }
}
