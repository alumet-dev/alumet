#ifndef __OUTPUT_H
#define __OUTPUT_H

#include <stdio.h>
#include "../../alumet/generated/alumet-api.h"

typedef struct {} StdOutput;

StdOutput *output_init();
void output_drop(StdOutput *output);
void output_write(StdOutput *output, const MeasurementBuffer *buffer);

#endif
