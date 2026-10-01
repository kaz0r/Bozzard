// Streamed structure models, automatic doors and local-player room cutaways.
import "factory-architecture" as architecture;
import "factory-grid" as grid;
import "factory-data" as data;
import "factory-net_input" as net;
fn board(name) {get_object_list("architecture-view",name)}
fn put(name,value) {set_object_list("architecture-view",name,value);}
fn field(name) {get_object_variable("architecture-view",name)}
fn set_field(name,value) {set_object_variable("architecture-view",name,value);}
fn region_text(name,region) {get_object_list_item("architecture-view",name,region)}
fn region_put(name,region,text) {let values=board(name);values.pad(289,"");values[region]=text;put(name,values);}
fn numbers(name,region,count) {grid::unpack_numbers(region_text(name,region),count)}
fn clear() {
    let loaded=board("loaded");
    for region in 0..loaded.len() {if loaded[region]!="" {unload(region);}}
    for name in ["loaded","rooms","door_progress","slots","kinds","doors"] {put(name,grid::empty_text(289));}
    for p in 0..12 {put("handles_"+p.to_string(),grid::empty_text(289));}
    put("active",[]);put("observed",[]);put("glimpses",[]);set_field("planet",-1.0);set_field("inside",0.0);set_field("view","");
    set_tile_view([],1.0,#{});
}
fn unload(region) {
    if region_text("loaded",region)=="" {return;}
    for p in 0..12 {
        let name="handles_"+p.to_string();
        for record in grid::unpack_text(region_text(name,region),75) {
            if record=="" {continue;}
            for target in record.split(",") {if target!="" {destroy_prefab(target);}}
        }
        region_put(name,region,"");
    }
    for name in ["loaded","door_progress","slots","kinds","doors"] {region_put(name,region,"");}
    let active=[];for id in board("active") {if id.to_int()!=region {active.push(id);}}put("active",active);
    set_field("view","");
}
fn edge_position(x,z,axis,slide) {
    if axis==0 {[x.to_float()+0.5,0.0,z.to_float()+slide]}else{[x.to_float()-slide,0.0,z.to_float()+0.5]}
}
fn load(region) {
    let pages=get_object_list("cache_structures");
    if pages.len()!=578 {return;}
    let encoded=pages[data::session_value(7).to_int()*289+region];
    if encoded==region_text("loaded",region) {return;}
    unload(region);
    let cells=grid::unpack_numbers(encoded,900);let slots=[];let kinds=[];let doors=[];
    for p in 0..12 {
        let records=grid::empty_text(75);
        for i in 0..75 {
            let slot=p*75+i;let kind=cells[slot];if kind==0.0 {continue;}
            slots.push(slot.to_float());kinds.push(kind);
            let id=region*225+slot/4;let x=grid::global_x(id).to_int();let z=grid::global_z(id).to_int();let layer=slot%4;
            let pos=if layer<2 {[x.to_float(),0.0,z.to_float()]}else{edge_position(x,z,layer-2,0.0)};
            let target=spawn_prefab("foundation-"+architecture::slug(kind),pos);records[i]=target;
            if layer>=2 {set_rotation(target,[0.0,-90.0*(layer-2).to_float(),0.0]);}
            if kind==38.0 {
                doors.push(slot.to_float());
                for side in [-1.0,1.0] {
                    let leaf=spawn_prefab("foundation-door-leaf",edge_position(x,z,layer-2,side*0.196));
                    set_rotation(leaf,[0.0,-90.0*(layer-2).to_float(),0.0]);records[i]+=","+leaf;
                }
            }
        }
        region_put("handles_"+p.to_string(),region,grid::pack_text(records));
    }
    region_put("loaded",region,encoded);
    region_put("slots",region,grid::pack_numbers(slots));region_put("kinds",region,grid::pack_numbers(kinds));region_put("doors",region,grid::pack_numbers(doors));
    if slots.len()>0 {let active=board("active");active.push(region.to_float());put("active",active);}
    set_field("view","");
}
fn build(kind,remove) {
    let direction=get_scene_variable("direction").to_int();
    if net::guest() {net::send("structure",#{kind:kind.to_int(),direction:direction,remove:remove});return;}
    let x=grid::world_x(get_scene_variable("cursor_x")).to_int();let z=grid::world_z(get_scene_variable("cursor_z")).to_int();
    let pages=get_object_list("cache_structures");
    let edit=architecture::edit(pages,data::session_value(7).to_int(),x,z,kind,direction,remove,get_scene_variable("seed").to_int(),get_scene_variable("demo_mode"),data::dev_world());
    if !edit.ok {set_scene_variable("message",edit.message);return;}
    if !remove && !data::pay_for_build(kind) {return;}
    grid::cache_put("cache_structures",edit.region,grid::pack_numbers(edit.data));
    load(edit.region);
    set_scene_variable("message",if remove {"Removed structure. Machines stay in place."}else{"Built "+data::build_name(kind)+"."});
}
fn players() {
    let positions=[[grid::world_x(get_scene_variable("cursor_x")).to_int(),grid::world_z(get_scene_variable("cursor_z")).to_int()]];
    if network_active() {
        for member in network_state().members {
            if member.position.planet==data::session_value(7).to_int() {positions.push([member.position.x,member.position.z]);}
        }
    }
    positions
}
fn update_doors(dt,resident) {
    if dt<=0.0 {return;}
    let positions=[];
    for entry in board("active") {
        let region=entry.to_int();
        if resident[region]==0.0 {continue;}
        let doors=numbers("doors",region,0);if doors.len()==0 {continue;}
        if positions.len()==0 {positions=players();}
        let progress=numbers("door_progress",region,doors.len());let changed=false;
        for i in 0..doors.len() {
                let slot=doors[i].to_int();
                let id=region*225+slot/4;let x=grid::global_x(id).to_int();let z=grid::global_z(id).to_int();let axis=slot%4-2;
                let near=false;
                for at in positions {
                    if min(abs(at[0]-x)+abs(at[1]-z),abs(at[0]-x-if axis==0 {1}else{0})+abs(at[1]-z-if axis==1 {1}else{0}))<=1 {near=true;}
                }
                // Blackboard pages store integers. Use millionths so packing
                // cannot stretch a 650 ms opening into a slower animation.
                let next=clamp((progress[i]+if near {dt*1000000.0/0.65}else{-dt*1000000.0/0.65}).round(),0.0,1000000.0);
                if abs(next-progress[i])<0.0001 {continue;}
                progress[i]=next;changed=true;
                let records=grid::unpack_text(region_text("handles_"+(slot/75).to_string(),region),75);
                let parts=records[slot%75].split(",");
                for side in 0..2 {
                    let delta=(if side==0 {-1.0}else{1.0})*(0.196+0.46*grid::smooth(next/1000000.0));
                    set_position(parts[side+1],edge_position(x,z,axis,delta));
                }
        }
        if changed {region_put("door_progress",region,grid::pack_numbers(progress));}
    }
}
fn can_cross(x,z,dir) {cross(x,z,dir,true)}
fn barrier(x,z,dir) {
    let a=architecture::address(x,z,32.0,dir);
    if a.region<0 || a.region>=289 {return 0.0;}
    let text=get_object_list_item("cache_structures",data::session_value(7).to_int()*289+a.region);
    if text=="" {return 0.0;}
    grid::unpack_cells(text,[a.slot.to_float()],1)[0]
}
fn cross(x,z,dir,announce) {
    let kind=barrier(x,z,dir);
    if kind==0.0 {return true;}
    if kind==38.0 {
        let a=architecture::address(x,z,kind,dir);
        let doors=numbers("doors",a.region,0);let index=doors.index_of(a.slot.to_float());
        if index>=0 && numbers("door_progress",a.region,doors.len())[index]>=1000000.0 {return true;}
        if announce {set_scene_variable("message","Wait for the sliding door to open.");}
    } else if announce {set_scene_variable("message",if kind==39.0 {"A window is a wall. Use the door."}else{"A wall blocks the way. Use the door."});}
    false
}
fn path_clear(x,z,nx,nz) {path_clear(x,z,nx,nz,true)}
fn path_clear(x,z,nx,nz,announce) {
    // Integer grid traversal checks both sides of diagonal corners. Pointer
    // selection cannot teleport through walls or a closed door.
    if x==nx && z==nz {return true;}
    let dx=abs(nx-x);let dz=abs(nz-z);let sx=if nx>x {1}else{-1};let sz=if nz>z {1}else{-1};
    let ix=0;let iz=0;
    while ix<dx || iz<dz {
        let a=(1+2*ix)*dz;let b=(1+2*iz)*dx;
        if a==b {
            let xd=if sx>0 {0}else{2};let zd=if sz>0 {1}else{3};
            if !cross(x,z,xd,announce) || !cross(x,z,zd,announce) || !cross(x+sx,z,zd,announce) || !cross(x,z+sz,xd,announce) {return false;}
            x+=sx;z+=sz;ix+=1;iz+=1;
        } else if a<b {if !cross(x,z,if sx>0 {0}else{2},announce) {return false;}x+=sx;ix+=1;}
        else {if !cross(x,z,if sz>0 {1}else{3},announce) {return false;}z+=sz;iz+=1;}
    }
    true
}
fn accessible(cell) {
    path_clear(grid::world_x(get_scene_variable("cursor_x")).to_int(),grid::world_z(get_scene_variable("cursor_z")).to_int(),
        grid::world_x(grid::cell_x(cell)).to_int(),grid::world_z(grid::cell_z(cell)).to_int(),false)
}
fn lighting() {set_sun_light([1.0,0.94,0.86],2.2);set_ambient_light([0.85,0.90,1.0],0.32);set_exposure(0.20);}
fn room_at(x,z) {
    let a=architecture::address(x,z,30.0,0);
    if a.region<0 || a.region>=289 {return 0;}
    grid::unpack_cells(region_text("rooms",a.region),[(a.slot/4).to_float()],1)[0].to_int()
}
fn visible_at(x,z) {
    let room=room_at(x,z);let inside=field("inside").to_int();
    if room==0 {return inside==0;}
    room==inside || board("glimpses").contains(architecture::key(x,z))
}
fn topology(pages) {
    let plan=architecture::topology(pages,data::session_value(7).to_int());
    let records=grid::empty_text(289);let values=#{};
    for key in plan.floors.keys() {
        let at=plan.floors[key];let region=at[3].to_string();
        if !values.contains(region) {values[region]=grid::empty_numbers(225);}
        values[region][at[4]]=plan.membership[key].to_float();
    }
    for key in values.keys() {records[parse_int(key)]=grid::pack_numbers(values[key]);}
    put("rooms",records);put("observed",pages);set_field("planet",data::session_value(7));set_field("view","");
}
fn update(dt) {
    let resident=get_object_list("resident");
    if !object_lists_equal("controller","cache_structures","architecture-view","observed") || field("planet")!=data::session_value(7) {
        let pages=get_object_list("cache_structures");if pages.len()!=578 {return;}
        for region in 0..289 {if resident[region]>0.0 {load(region);}}
        topology(pages);
    }
    update_doors(dt,resident);
    let x=grid::world_x(get_scene_variable("cursor_x")).to_int();let z=grid::world_z(get_scene_variable("cursor_z")).to_int();
    let inside=room_at(x,z);set_field("inside",inside.to_float());
    let heading=get_scene_variable("camera_heading");
    let signature=[data::session_value(7),x.to_float(),z.to_float(),inside.to_float(),heading].to_string();
    if signature==field("view") {
        if inside>0 {lighting();}
        return;
    }
    let exterior=if inside>0 {0.10}else{1.0};let mask=#{};let overrides=#{ground:exterior,cursor:1.0,"hover-tile":1.0};let glimpses=[];
    let regions=board("active");let room_values=#{};
    for entry in regions {
        let region=entry.to_int();
        for neighbor in [region,region-1,region+1,region-17,region+17] {
            if neighbor<0 || neighbor>=289 {continue;}
            let key=neighbor.to_string();if !room_values.contains(key) {room_values[key]=numbers("rooms",neighbor,225);}
        }
    }
    // Windows expose at most two floor tiles down their inward sight line.
    if inside==0 {
        for entry in regions {
            let region=entry.to_int();let slots=numbers("slots",region,0);let pieces=numbers("kinds",region,0);
            for i in 0..slots.len() {
                if pieces[i]!=39.0 {continue;}let slot=slots[i].to_int();
                let id=region*225+slot/4;let wx=grid::global_x(id).to_int();let wz=grid::global_z(id).to_int();let axis=slot%4-2;
                for sign in [-1,1] {
                    let sx=if axis==0 {sign}else{0};let sz=if axis==1 {sign}else{0};
                    let outx=wx+if sign<0 && axis==0 {1}else{0};let outz=wz+if sign<0 && axis==1 {1}else{0};
                    if room_at(outx,outz)>0 || abs(x-outx)+abs(z-outz)>2 {continue;}
                    for depth in 1..3 {
                        let tx=outx+sx*depth;let tz=outz+sz*depth;
                        if room_at(tx,tz)==0 {break;}
                        if depth>1 && barrier(tx-sx,tz-sz,if axis==0 {if sign>0 {0}else{2}}else{if sign>0 {1}else{3}})>0.0 {break;}
                        let key=architecture::key(tx,tz);mask[key]=1.0;glimpses.push(key);
                    }
                }
            }
        }
    }
    let angle=heading*0.01745329252;let camera_x=cos(angle)+sin(angle);let camera_z=cos(angle)-sin(angle);
    for entry in regions {
        let region=entry.to_int();let slots=numbers("slots",region,0);
        let ids=room_values[region.to_string()];
        for value in slots {
            let slot=value.to_int();if slot%4!=0 {continue;}let cell=slot/4;
            if ids[cell]==0.0 {continue;}
            let id=region*225+cell;let key=architecture::key(grid::global_x(id).to_int(),grid::global_z(id).to_int());
            if ids[cell].to_int()==inside {mask[key]=1.0;}else if !mask.contains(key){mask[key]=0.0;}
        }
        let handles=#{};
        for value in slots {
                let slot=value.to_int();if slot%4==0 {continue;}
                let id=region*225+slot/4;let wx=grid::global_x(id).to_int();let wz=grid::global_z(id).to_int();let layer=slot%4;
                let key=architecture::key(wx,wz);let factor=exterior;
                if layer==1 {
                    if mask.contains(key) && mask[key]==1.0 {factor=0.0;}
                } else if layer>=2 {
                    let axis=layer-2;let a=ids[slot/4].to_int();
                    let neighbor=architecture::address(wx+if axis==0 {1}else{0},wz+if axis==1 {1}else{0},30.0,0);
                    let b=if room_values.contains(neighbor.region.to_string()) {room_values[neighbor.region.to_string()][neighbor.slot/4].to_int()}else{0};
                    if inside>0 && (a==inside || b==inside) {
                        factor=1.0;
                        let camera=if axis==0 {camera_x}else{camera_z};
                        if (camera>0.001 && a==inside && b!=inside) || (camera < -0.001 && b==inside && a!=inside) {factor=0.0;}
                    }
                }
                let page=(slot/75).to_string();if !handles.contains(page) {handles[page]=grid::unpack_text(region_text("handles_"+page,region),75);}
                for target in handles[page][slot%75].split(",") {overrides[target]=factor;}
        }
    }
    let cells=[];
    for key in mask.keys() {let pos=key.split(",");cells.push(parse_int(pos[0]).to_float());cells.push(parse_int(pos[1]).to_float());cells.push(mask[key]);}
    set_tile_view(cells,exterior,overrides);put("glimpses",glimpses);set_field("view",signature);
    if inside>0 {lighting();}
}
