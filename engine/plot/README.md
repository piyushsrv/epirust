# Plot the simulations

## Prerequisites
* Python3
* Matplotlib
* Pandas
* Numpy


## Plot one particular simulation

To visualise one particular simulation:
```bash
python plot.py --data-path <PATH_TO_CSV_FILE>
```

Sample output:
![Epicurves plot](https://user-images.githubusercontent.com/16804955/77611863-789d6500-6f4c-11ea-9a8d-5cc130f54abd.png)

# Plot the average of all simulations

To calculate the average of all the simulations and visualise it:
```bash
python collate_all_simulations.py --data-path <PATTERN_TO_CSVS>
```

Example:
```bash
python collate_all_simulations.py --data-path ./simulation*.csv
```

Or you can pass particular files as well
Example:
```bash
python collate_all_simulations.py --data-path ./simulation1.csv ./simulation2.csv
```

Sample output:
![Epicurves plot](https://user-images.githubusercontent.com/16804955/77741322-b8427a80-703a-11ea-896f-9d0235927b94.png)

The shaded region represents the standard deviation around the mean of the curve at one particular time

To save the calculated mean and standard deviation to csv file:
```bash
python collate_all_simulations.py --data-path <PATTERN_TO_CSVS> --output-path <PATH_TO_OUTPUT_CSV>
```

Example:
```bash
python collate_all_simulations.py --data-path ./simulation*.csv --output-path ./mean_simulation.csv
```

Note: if `--output-path` is a directory e.g `./` then the file saved would be `./collated_simulation.csv`

### Plot already calculated csv file

To visualise already generated average and standard deviation calculated csv:
```bash
python collate_all_simulations.py --collated-csv <PATH_TO_COLLATED_CSV>
```

Example:
```bash
python collate_all_simulations.py --collated-csv ./mean_simulation.csv
```