/*
 * EpiRust
 * Copyright (c) 2020  ThoughtWorks, Inc.
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 */

use core::borrow::Borrow;
use core::borrow::BorrowMut;
use std::collections::HashMap;
use std::time::{Instant, SystemTime, Duration};

use chrono::{DateTime, Local};
use fxhash::{FxBuildHasher, FxHashMap};
use rand::Rng;

use crate::{allocation_map, constants, ticks_consumer, RunMode};
use crate::allocation_map::AgentLocationMap;
use crate::config::{Config, Population};
use crate::disease::Disease;
use crate::geography;
use crate::geography::Grid;
use crate::interventions::{Intervention, Lockdown};
use crate::listeners::csv_service::CsvListener;
use crate::listeners::disease_tracker::Hotspot;
use crate::listeners::events::counts::Counts;
use crate::listeners::events_kafka_producer::EventsKafkaProducer;
use crate::listeners::listener::Listeners;
use crate::random_wrapper::RandomWrapper;
use crate::kafka_producer::{KafkaProducer, TickAck};
use futures::StreamExt;

pub struct Epidemiology {
    pub agent_location_map: allocation_map::AgentLocationMap,
    pub write_agent_location_map: allocation_map::AgentLocationMap,
    pub grid: Grid,
    pub disease: Disease,
    pub sim_id: String,
}

impl Epidemiology {
    pub fn new(config: &Config, sim_id: String) -> Epidemiology {
        let start = Instant::now();
        let disease = config.get_disease();
        let grid = geography::define_geography(config.get_grid_size());
        let mut rng = RandomWrapper::new();
        let (start_locations, agent_list) = match config.get_population() {
            Population::Csv(csv_pop) => grid.read_population(&csv_pop, &mut rng),
            Population::Auto(auto_pop) => grid.generate_population(&auto_pop, &mut rng),
        };

        let agent_location_map = allocation_map::AgentLocationMap::new(config.get_grid_size(), &agent_list, &start_locations);
        let write_agent_location_map = allocation_map::AgentLocationMap::new(config.get_grid_size(), &agent_list, &start_locations);

        println!("Initialization completed in {} seconds", start.elapsed().as_secs_f32());
        Epidemiology { agent_location_map, write_agent_location_map, grid, disease, sim_id }
    }

    fn stop_simulation(row: Counts) -> bool {
        row.get_infected() == 0 && row.get_quarantined() == 0
    }

    pub async fn run(&mut self, config: &Config, run_mode: &RunMode) {
        let now: DateTime<Local> = SystemTime::now().into();
        let output_file_prefix = config.get_output_file().unwrap_or("simulation".to_string());
        let output_file_name = format!("{}_{}.csv", output_file_prefix, now.format("%Y-%m-%dT%H:%M:%S"));
        let csv_listener = CsvListener::new(output_file_name);
        let kafka_listener = EventsKafkaProducer::new(self.sim_id.clone(), self.agent_location_map.agent_cell.len(),
                                                      config.enable_citizen_state_messages());
        let hotspot_tracker = Hotspot::new();
        let mut listeners = Listeners::from(vec![Box::new(csv_listener), Box::new(kafka_listener), Box::new(hotspot_tracker)]);

        let mut counts_at_hr = Counts::new((self.agent_location_map.agent_cell.len() - 1) as i32, 1);
        let mut rng = RandomWrapper::new();
        let start_time = Instant::now();

        self.write_agent_location_map.agent_cell = FxHashMap::with_capacity_and_hasher(self.agent_location_map.agent_cell.len(), FxBuildHasher::default());

        let vaccinations = Epidemiology::prepare_vaccinations(config);
        let lock_down_details = Intervention::get_lock_down_intervention(config);
        let hospital_intervention = Intervention::get_hospital_intervention(config);
        let mut infection_count_for_yesterday = 0;
        let mut city_to_be_locked_till: i32 = 0;
        let mut is_city_locked_down = false;

        listeners.grid_updated(&self.grid);
        let mut producer = KafkaProducer::new();

        //todo stream should be started only in case of multi-sim mode
        let engine_id = if let RunMode::MultiEngine {engine_id} = run_mode {
            engine_id
        } else {
            "n_a"
        };
        let consumer = ticks_consumer::start(engine_id);
        let mut message_stream = consumer.start_with(Duration::from_millis(10), false);

        for simulation_hour in 1..config.get_hours() {
            if let RunMode::MultiEngine { engine_id } = run_mode {
                let msg = message_stream.next().await;
                let clock_tick = ticks_consumer::read(msg);
                println!("{:?}", clock_tick);
                if clock_tick.is_none() {
                    break;
                }
                let clock_tick = clock_tick.unwrap();
                match producer.send_ack(TickAck { engine_id: engine_id.to_string(), hour: clock_tick }).await.unwrap() {
                    Ok(_) => {
                        if clock_tick == config.get_hours() {
                            break;
                        }
                    }
                    Err(_) => panic!("Failed while sending acknowledgement")
                }
            }

            counts_at_hr.increment_hour();
            let start_of_day = simulation_hour % 24 == 0;

            let mut read_buffer_reference = self.agent_location_map.borrow();
            let mut write_buffer_reference = self.write_agent_location_map.borrow_mut();

            if simulation_hour % 2 == 0 {
                read_buffer_reference = self.write_agent_location_map.borrow();
                write_buffer_reference = self.agent_location_map.borrow_mut();
            }

            if start_of_day {
                let rate_of_spread = counts_at_hr.get_infected() - infection_count_for_yesterday;
                match hospital_intervention {
                    Some(x) if rate_of_spread >= x.spread_rate_threshold => {
                        println!("Increasing the hospital size");
                        self.grid.increase_hospital_size(config.get_grid_size());

                        listeners.grid_updated(&self.grid);
                    }
                    _ => {}
                }
            }

            Epidemiology::simulate(&mut counts_at_hr, simulation_hour, read_buffer_reference, write_buffer_reference,
                                   &self.grid, &mut listeners, &mut rng, &self.disease);
            listeners.counts_updated(counts_at_hr);

            match lock_down_details {
                Some(x) if Epidemiology::should_lock_city(&counts_at_hr, is_city_locked_down, x) => {
                    Epidemiology::lock_city(&mut write_buffer_reference, &mut rng, &x);
                    is_city_locked_down = true;
                    city_to_be_locked_till = simulation_hour + x.lock_down_period * constants::NUMBER_OF_HOURS;
                }
                _ => {}
            }

            if is_city_locked_down && city_to_be_locked_till == simulation_hour {
                Epidemiology::unlock_city(&mut write_buffer_reference);
            }

            match vaccinations.get(&simulation_hour) {
                Some(vac_percent) => {
                    println!("Vaccination");
                    Epidemiology::vaccinate(*vac_percent, &mut write_buffer_reference, &mut rng);
                }
                _ => {}
            };

            if Epidemiology::stop_simulation(counts_at_hr) {
                break;
            }

            if simulation_hour % 100 == 0 {
                println!("Throughput: {} iterations/sec; simulation hour {} of {}",
                         simulation_hour as f32 / start_time.elapsed().as_secs_f32(),
                         simulation_hour, config.get_hours());
            }

            if start_of_day {
                infection_count_for_yesterday = counts_at_hr.get_infected();
            }
        }
        let elapsed_time = start_time.elapsed().as_secs_f32();
        println!("Number of iterations: {}, Total Time taken {} seconds", counts_at_hr.get_hour(), elapsed_time);
        println!("Iterations/sec: {}", counts_at_hr.get_hour() as f32 / elapsed_time);
        listeners.simulation_ended();
    }

    fn should_lock_city(counts_at_hr: &Counts, is_city_locked_down: bool, x: Lockdown) -> bool {
        !is_city_locked_down && (counts_at_hr.get_infected() > x.at_number_of_infections)
    }

    fn prepare_vaccinations(config: &Config) -> HashMap<i32, f64> {
        let mut vaccinations: HashMap<i32, f64> = HashMap::new();
        config.get_interventions().iter().filter_map(|i| {
            match i {
                Intervention::Vaccinate(v) => Some(v),
                _ => None,
            }
        }).for_each(|v| {
            vaccinations.insert(v.at_hour, v.percent);
        });
        vaccinations
    }

    fn vaccinate(vaccination_percentage: f64, write_buffer_reference: &mut AgentLocationMap, rng: &mut RandomWrapper) {
        for (_v, agent) in write_buffer_reference.agent_cell.iter_mut() {
            if agent.is_susceptible() && rng.get().gen_bool(vaccination_percentage) {
                agent.set_vaccination(true);
            }
        }
    }

    fn simulate(mut csv_record: &mut Counts, simulation_hour: i32, read_buffer: &AgentLocationMap,
                write_buffer: &mut AgentLocationMap, grid: &Grid, listeners: &mut Listeners,
                rng: &mut RandomWrapper, disease: &Disease) {
        write_buffer.agent_cell.clear();
        for (cell, agent) in read_buffer.agent_cell.iter() {
            let mut current_agent = *agent;
            let infection_status = current_agent.is_infected();
            let point = current_agent.perform_operation(*cell, simulation_hour, &grid, read_buffer, &mut csv_record, rng, disease);

            if infection_status == false && current_agent.is_infected() == true {
                listeners.citizen_got_infected(&cell);
            }

            let agent_option = write_buffer.agent_cell.get(&point);
            let new_location = match agent_option {
                Some(mut _agent) => cell, //occupied
                _ => &point
            };
            write_buffer.agent_cell.insert(*new_location, current_agent);
            listeners.citizen_state_updated(simulation_hour, &current_agent, new_location);
        }
    }

    fn lock_city(write_buffer_reference: &mut AgentLocationMap, rng: &mut RandomWrapper, lockdown_details: &Lockdown) {
        println!("Locking the city");
        for (_v, agent) in write_buffer_reference.agent_cell.iter_mut() {
            if rng.get().gen_bool(1.0 - lockdown_details.essential_workers_population) {
                agent.set_isolation(true);
            }
        }
    }

    fn unlock_city(write_buffer_reference: &mut AgentLocationMap) {
        println!("unlocking city");
        for (_v, agent) in write_buffer_reference.agent_cell.iter_mut() {
            if agent.is_isolated() {
                agent.set_isolation(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::AutoPopulation;
    use crate::geography::Area;
    use crate::geography::Point;
    use crate::interventions::Vaccinate;

    use super::*;

    #[test]
    fn should_init() {
        let pop = AutoPopulation {
            number_of_agents: 10,
            public_transport_percentage: 1.0,
            working_percentage: 1.0,
        };
        let disease = Disease::new(0, 0, 0, 0.0, 0.0, 0.0);
        let vac = Vaccinate {
            at_hour: 5000,
            percent: 0.2,
        };
        let config = Config::new(Population::Auto(pop), disease, vec![], 100, 10000,
                                 vec![Intervention::Vaccinate(vac)], None);
        let epidemiology: Epidemiology = Epidemiology::new(&config, "id".to_string());
        let expected_housing_area = Area::new(Point::new(0, 0), Point::new(40, 100));
        assert_eq!(epidemiology.grid.housing_area, expected_housing_area);

        let expected_transport_area = Area::new(Point::new(40, 0), Point::new(50, 100));
        assert_eq!(epidemiology.grid.transport_area, expected_transport_area);

        let expected_work_area = Area::new(Point::new(50, 0), Point::new(70, 100));
        assert_eq!(epidemiology.grid.work_area, expected_work_area);

        let expected_hospital_area = Area::new(Point::new(70, 0), Point::new(80, 100));
        assert_eq!(epidemiology.grid.hospital_area, expected_hospital_area);

        assert_eq!(epidemiology.agent_location_map.agent_cell.len(), 10);
    }
}
