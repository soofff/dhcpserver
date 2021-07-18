# DHCP Server
* multiple sources
* yaml configuration
* multi threaded

```
                                       +------------------------+
                                       |                        |
                                       |         Client         |
                                       |                    |   |
                                       +--------------------+---+
                                                            |
                                                            |
                                                            |
+----------------------+    +-------------------------------+--------------------+
|                      |    |                               |                    |
|                      |    |   +---------------------------+----------------+   |
|   Source Backend     |    |   |                           |                |   |
|                      |    |   |   Server (listener)       v                |   |
|                      |    |   |                                            |   |
|                      |    |   |        |                                   |   |
+----------^-----------+    |   +--------+-----------------------------------+   |
           |                |            |                                       |
           |                |            |                                       |
           |                |  +---------+---------+  +----------------------+   |
           |                |  |         v         |  |                      |   |
           |                |  |  REST Source      |  |  config.yml          |   |
           +----------------+--+-                  |  |                      |   |
                            |  |                   |  |                      |   |
                            |  +-------------------+  +----------------------+   |
                            |                                                    |
                            +----------------------------------------------------+
```


## Configuration
* command line help and options `-h` 
* [config.file example](config.yml.example)


## Sources
* Configurable sources

| name          | description                                                   |
|---------------|---------------------------------------------------------------|
| rest          | get hosts and options from rest backend                       |

### HTTP REST
* query multiple http requests
* templating by https://github.com/Keats/tera (jinja like)
* map dhcp options from query result
* custom dhcp options
* run executable scripts/programs while sending dhcp packet
* response is expected as json

#### Templating
* results are stored with format: `result.<query name>.<key path>`

##### variables
| name                              | description                                                   |
|-----------------------------------|---------------------------------------------------------------|
| client_hardware_address           | client mac address - always available                         |
| client_ip_address                 | client ip address                                             |
| client_hostname                   | client hostname                                               |
| server_ip_address                 | server ip - always available                                  |

##### mapping
##### format
```yaml
<dhcp_option_name>:
  data: <option data>
  required: <can be ignored on error or missing data>
```

###### available options
| name                              |
|-----------------------------------|
| subnet_mask |
| time_offset |
| router |
| time_server |
| domain_name_server |
| log_server |
| cookie_server |
| lpr_server |
| impress_server |
| resource_location_server |
| host_name |
| boot_file_size |
| merit_dump_file |
| domain_name |
| swap_server |
| root_path |
| extension_path |
| ip_forwarding |
| non_local_source_routing |
| policy_filter |
| maximum_datagram_reassembly_size |
| default_ip_ttl |
| path_mtu_aging_timeout |
| path_mtu_plateau_table |
| interface_mtu |
| all_subnets_local |
| broadcast_address |
| mask_supplier |
| perform_router_discovery |
| router_solicitation_address |
| static_route |
| trailer_encapsulation |
| arp_cache_timeout |
| ethernet_encapsulation |
| tcp_default_ttl |
| tcp_keep_alive_interval |
| tcp_keep_alive_garbage |
| network_information_service_domain |
| network_information_servers |
| network_time_protocol_servers |
| net_bios_over_tcp_ip_name_server |
| net_bios_over_tcp_ip_datagram_distribution_server |
| net_bios_over_tcp_ip_node_type |
| net_bios_over_tcp_ip_scope |
| x_window_system_font_server |
| x_window_system_display_manager |
| requested_ip_address |
| ip_address_lease_time |
| option_overload |
| message_type |
| server_identifier |
| parameter_request_list |
| message |
| maximum_dhcp_message_size |
| renewal_time_value |
| rebinding_time_value |
| vendor_class_identifier |
| client_identifier |
| network_information_service_plus_domain |
| network_information_service_plus_server |
| tftp_server |
| boot_file_name |
| mobile_ip_home_agent |
| smtp_server |
| pop3_server |
| nntp_server |
| www_server |
| finger_server |
| irc_server |
| street_talk_server |
| street_talk_directory_assistance_server |

##### custom option
```yaml
<custom_option_name>:
  data: <option data>
  required: <can be ignored on error or missing data>
  tag: <option number>
  kind: <string/integer/bool>
```
